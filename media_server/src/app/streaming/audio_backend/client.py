import asyncio

from pydantic import ValidationError

from .comm_protocol_models import (
    ResponseValidator,
    IssueCommand,
    CloseCommand,
    HandshakeMessage,
    HandshakeParseError,
    StreamMetadata,
    SuccessResponse,
    Commands,
)


class AudioBackendError(Exception):
    """Base error for all audio backend client operations."""

    def __init__(self, message: str, cause: Exception | None = None):
        super().__init__(message)
        self.cause: Exception | None = cause


class BackendConnectionError(AudioBackendError):
    """Failed to establish TCP connection."""

    pass


class BackendProtocolError(AudioBackendError):
    """Connection established, but protocol handshake failed."""

    pass


class BackendDisconnectedError(AudioBackendError):
    """Remote closed connection unexpectedly."""

    pass


class BackendInvalidResponse(AudioBackendError):
    """Response from backend was invalid."""

    pass


class BackendIssueWorkerError(AudioBackendError):
    """Backend returned with an error on issue cmd"""

    pass


class HandshakeResponseParsingError(AudioBackendError):
    """Backend has returned invalid response."""

    pass


class AudioBackendClient:
    _rust_socket_reader: asyncio.StreamReader
    _rust_socket_writer: asyncio.StreamWriter
    stream_metadata: StreamMetadata

    def __init__(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        metadata: StreamMetadata,
    ):
        self._rust_socket_reader = reader
        self._rust_socket_writer = writer
        self.stream_metadata = metadata

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        await self.close()

    @classmethod
    async def connect(cls, host: str, port: int) -> "AudioBackendClient":
        """
        Raise:
            BackendDisconnectedError on failure to establish connection or socket write
            HandshakeResponseParsingError on failure to parse handshake response
        """
        try:
            (reader, writer) = await asyncio.open_connection(host, port)

            handshake_bytes = await reader.readline()
            if not handshake_bytes:
                writer.close()
                await writer.wait_closed()
                raise BackendDisconnectedError(
                    "Failed to read the handshake response for the Rust backend."
                )

            handshake = HandshakeMessage.from_bytes(handshake_bytes)
            return cls(reader, writer, handshake.stream_metadata)
        except OSError as e:
            raise BackendDisconnectedError(
                "Failed to establish connection or read on the rust socket."
            ) from e
        except HandshakeParseError as e:
            raise HandshakeResponseParsingError(
                "Failed to parse the handshake response from a backend."
            ) from e

    async def close(self):
        if not self._rust_socket_writer.is_closing():
            self._rust_socket_writer.close()
            await self._rust_socket_writer.wait_closed()

    async def _send_command(self, command: Commands):
        try:
            cmd = command.model_dump_json().encode("utf-8") + b"\n"
            self._rust_socket_writer.write(cmd)
            await self._rust_socket_writer.drain()
        except OSError as e:
            raise BackendDisconnectedError("Failed to write on the rust socket.") from e

    async def issue_new_worker(self) -> int:
        """
        Raise:
            BackendDisconnectedError on failed socket read or write,
            BackendIssueWorkerError on iternal rust backend error
        """
        try:
            await self._send_command(IssueCommand(cmd="issue"))

            response_bytes = await self._rust_socket_reader.readline()
            response = ResponseValidator.validate_json(response_bytes)

            if isinstance(response, SuccessResponse):
                return response.port
            else:
                raise BackendIssueWorkerError(
                    f"Rust backend has returned with an error: {response.message}"
                )
        except ValidationError as e:
            raise BackendInvalidResponse(f"Invalid response format: {e}") from e
        except OSError as e:
            raise BackendDisconnectedError("Failed to read on the rust socket.") from e

    async def drop_worker(self, port: int) -> int:
        """
        Raise:
            BackendDisconnectedError on failed socket read or write,
            BackendIssueWorkerError on iternal rust backend error
        """
        try:
            await self._send_command(CloseCommand(cmd="close", port=port))

            response_bytes = await self._rust_socket_reader.readline()
            response = ResponseValidator.validate_json(response_bytes)

            if isinstance(response, SuccessResponse):
                return response.port
            else:
                raise BackendIssueWorkerError(
                    f"Rust backend has returned with an error: {response.message}"
                )
        except OSError as e:
            raise BackendDisconnectedError("Failed to read on the rust socket.") from e
