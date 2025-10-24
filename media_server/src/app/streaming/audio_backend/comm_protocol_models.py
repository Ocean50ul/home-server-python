from __future__ import annotations
from typing import Literal, Annotated

from pydantic import BaseModel, ValidationError, Field, TypeAdapter


# Model for: { "cmd": "issue" }
class IssueCommand(BaseModel):
    cmd: Literal["issue"]


# Model for: { "cmd": "close", "port": <u16> }
class CloseCommand(BaseModel):
    cmd: Literal["close"]
    port: Annotated[int, Field(strict=True, ge=0, le=65535)]


# Model for: { "status": "success", "type:" "closed", "port": <u16> }
class ClosedSuccess(BaseModel):
    """Response to a `Close` command."""

    status: Literal["success"]
    type: Literal["closed"]
    port: Annotated[int, Field(strict=True, ge=0, le=65535)]


# Model for: { "status": "error", "message": str }
class ErrorResponse(BaseModel):
    """A generic error response."""

    status: Literal["error"]
    message: str


# Model for { "status": "success", "type:" "issued", "port": <u16> }
class IssuedSuccess(BaseModel):
    """Response to an `Issue` command."""

    status: Literal["success"]
    type: Literal["issued"]
    port: Annotated[int, Field(strict=True, ge=0, le=65535)]


Commands = IssueCommand | CloseCommand
SuccessResponse = IssuedSuccess | ClosedSuccess
Response = SuccessResponse | ErrorResponse
ResponseValidator: TypeAdapter[Response] = TypeAdapter(Response)


# Model for: { "sample_rate": 48000, "channels": 2, "format": "f32"}
class StreamMetadata(BaseModel):
    sample_rate: int
    channels: int
    format: str


class HandshakeParseError(Exception):
    """Base exception for errors during handshake message parsing from bytes."""

    def __init__(self, message: str, original_exception: Exception):
        super().__init__(message)
        self.original_exception: Exception = original_exception


class HandshakeUnicodeError(HandshakeParseError):
    """Raised when the byte sequence cannot be decoded as UTF-8."""

    pass


class HandshakeValidationError(HandshakeParseError):
    """Raised when the decoded JSON is invalid for a HandshakeMessage."""

    pass


# Model for: { "stream_metadata": { "sample_rate": <int>, "channels": <int>, "format": str} }
class HandshakeMessage(BaseModel):
    stream_metadata: StreamMetadata

    @classmethod
    def from_jstring(cls, jstring: str) -> HandshakeMessage:
        """
        Raise: HandshakeValidationError on fail to validate json string
        """
        try:
            return cls.model_validate_json(jstring)
        except ValidationError as e:
            raise HandshakeValidationError(
                "JSON validation failed", original_exception=e
            )

    @classmethod
    def from_bytes(cls, data: bytes) -> HandshakeMessage:
        """
        Raise:
            HandshakeUnicodeError on fail to decode bytes
            HandshakeValidationError on fail to validate json string
        """
        try:
            s_message = data.decode("utf-8").strip()
            return cls.from_jstring(s_message)
        except UnicodeDecodeError as e:
            raise HandshakeUnicodeError(
                "Failed to decode bytes as UTF-8", original_exception=e
            )
