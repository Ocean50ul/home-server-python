from asyncio import StreamReader, StreamWriter, Task, create_task, IncompleteReadError
from enum import Enum
import struct
import logging

from fastapi import WebSocket, WebSocketDisconnect
from websockets import ConnectionClosed

logger = logging.getLogger(__name__)


class SessionError(Exception):
    pass


class SessionClosingError(SessionError):
    pass


class SessionTaskError(Exception):
    pass


class WorkerDisconnectedError(SessionTaskError):
    port: int

    def __init__(self, message: str, port: int, cause: Exception | None = None):
        super().__init__(message)
        self.port: int = port
        self.cause: Exception | None = cause


class HeaderUnpackingError(SessionTaskError):
    pass


class WebsocketConnectionClosed(SessionTaskError):
    pass


class SessionState(Enum):
    RUNNING = "running"
    STOPPING = "stopping"
    STOPPED = "stopped"
    ERROR = "error"


class Session:
    encoding_worker_reader: StreamReader
    encoding_worker_writer: StreamWriter
    websocket: WebSocket
    state: SessionState
    task: Task | None
    worker_port: int

    def __init__(
        self,
        encoding_worker_reader: StreamReader,
        encoding_worker_writer: StreamWriter,
        worker_port: int,
        websocket: WebSocket,
    ) -> None:
        self.encoding_worker_reader = encoding_worker_reader
        self.encoding_worker_writer = encoding_worker_writer
        self.websocket = websocket
        self.state = SessionState.STOPPED
        self.task = None
        self.worker_port = worker_port

    async def start(self, on_done_callback: callable) -> None:
        """
        Raises:
            None
        """
        self.task = create_task(self._run_session())
        self.task.add_done_callback(on_done_callback)

    async def close(self) -> None:
        """
        Raises:
            None
        """
        self.state = SessionState.STOPPING
        if self.task and not self.task.done():
            self.task.cancel()

            try:
                await self.task
            except Exception as e:
                logger.info(
                    f"Error during awaitng the the task while closing the session: {e}"
                )

        self.state = SessionState.STOPPED

        if not self.encoding_worker_writer.is_closing():
            self.encoding_worker_writer.close()

            try:
                await self.encoding_worker_writer.wait_closed()
            except Exception as e:
                logger.info(
                    f"Error during awaiting closing encoder worker connection: {e}"
                )

    async def _run_session(self):
        self.state = SessionState.RUNNING
        while True:
            try:
                header = await self.encoding_worker_reader.readexactly(4)
                (frame_len,) = struct.unpack("!I", header)
                frame = await self.encoding_worker_reader.readexactly(frame_len)

                await self.websocket.send_bytes(bytes(frame))
            except IncompleteReadError as e:
                await self.close()
                raise WorkerDisconnectedError(
                    f"Failed to read header or body from workers port {self.worker_port}: {e}",
                    port=self.worker_port,
                ) from e
