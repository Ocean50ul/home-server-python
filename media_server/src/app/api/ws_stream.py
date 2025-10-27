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

    session: Session | None = None
    writer: asyncio.StreamWriter | None = None
    worker_port: int | None = None

    try:
        while True:
            message = await websocket.receive_json()

            if message.get("command") == "start":
                if session:
                    continue

                worker_port = await backend_client.issue_new_worker()
                print(
                    f"Received start command from the client. Issued new worker on port {worker_port}"
                )

                (reader, writer) = await asyncio.open_connection(
                    "127.0.0.1", worker_port
                )
                session = Session(reader, websocket)

                await session.start()

                print("Session started. Streaming audio...")

            if message.get("command") == "stop":
                if not session:
                    continue

                print(
                    f"Received stop command from the client. Closing the worker on port {worker_port} and cleaning up the session."
                )
                await session.close()

                if writer and not writer.is_closing():
                    writer.close()

                if worker_port:
                    dropped_port = await backend_client.drop_worker(worker_port)
                    print(f"Dropped worker on port {dropped_port}.")

                print("Session closed.")

    except WebSocketDisconnect:
        print("Client has disconnected.")
    except Exception as e:
        print(f"Some kind of a exception occured: {e}. Stupid fucking mistakes.")
    finally:
        print("Cleaning up the session on finally...")
        await session.close()

        if writer and not writer.is_closing():
            writer.close()

        if worker_port:
            dropped_port = await backend_client.drop_worker(worker_port)
            print(f"Dropped worker on port {dropped_port}")

        print("Cleanup complete.")
