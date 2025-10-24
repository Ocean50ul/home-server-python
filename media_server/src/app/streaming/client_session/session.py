import asyncio
from enum import Enum
import struct

from fastapi import WebSocket


class SessionState(Enum):
    PLAYING = "playing"
    STOPPED = "stopped"


class Session:
    opus_stream_reader: asyncio.StreamReader
    websocket: WebSocket
    state: SessionState
    buff: bytearray

    def __init__(
        self,
        opus_stream_reader: asyncio.StreamReader,
        websocket: WebSocket,
    ) -> None:
        self.opus_stream_reader = opus_stream_reader
        self.websocket = websocket
        self.state = SessionState.STOPPED
        self.buff = bytearray()

    async def _recv_all(self, n: int) -> bytearray:
        data = bytearray()

        while len(data) < n:
            packet = await self.opus_stream_reader.read(n - len(data))
            data.extend(packet)

        return data

    async def start(self):
        while True:
            header_data = await self.opus_stream_reader.readexactly(4)
            (frame_len,) = struct.unpack("!I", header_data)
            frame = await self.opus_stream_reader.readexactly(frame_len)

            await self.websocket.send_bytes(bytes(frame))

    async def close(self) -> None:
        pass
