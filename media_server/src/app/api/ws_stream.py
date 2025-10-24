import asyncio

from fastapi import APIRouter, WebSocket, Depends, WebSocketDisconnect

from app.streaming import Session, AudioBackendClient
from .dependencies import get_backend_client

router = APIRouter()


@router.websocket("/ws/audio")
async def websocket_endpoint(
    websocket: WebSocket,
    backend_client: AudioBackendClient = Depends(get_backend_client),
):
    await websocket.accept()

    worker_port = None
    writer = None
    session_task = None

    try:
        message = await websocket.receive_json()

        if message.get("command") == "start":
            worker_port = await backend_client.issue_new_worker()
            print(f"Issued new worker on port {worker_port}")

            (reader, writer) = await asyncio.open_connection("127.0.0.1", worker_port)
            session = Session(reader, websocket)

            session_task = asyncio.create_task(session.start())

            print("Session started. Streaming audio...")

            while True:
                await websocket.receive_json()

    except WebSocketDisconnect:
        print("Client has disconnected.")
    except Exception as e:
        print(f"Some kind of a exception occured: {e}. Stupid fucking mistakes.")
    finally:
        print("Cleaning up session...")

        if session_task and not session_task.done():
            session_task.cancel()

        if writer and not writer.is_closing():
            writer.close()

        if worker_port:
            await backend_client.drop_worker(worker_port)
            print(f"Dropped worker on port {worker_port}")

        print("Cleanup complete.")
