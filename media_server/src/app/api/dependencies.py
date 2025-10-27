from fastapi import Request, WebSocket
from app.streaming.sessions_manager.sessions_manager import SessionsManager


async def get_sessions_manager(websocket: WebSocket) -> SessionsManager:
    return websocket.app.state.backend_client
