from fastapi import Request, WebSocket
from app.streaming.audio_backend import AudioBackendClient


async def get_backend_client(websocket: WebSocket) -> AudioBackendClient:
    return websocket.app.state.backend_client
