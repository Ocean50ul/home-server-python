from pathlib import Path
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles

from app.api import healthz, ws_stream
from app.streaming.audio_backend import AudioBackendClient

# src/app/
CURRENT_DIR = Path(__file__).resolve().parent

# media-server/
PROJECT_ROOT = CURRENT_DIR.parent.parent

# media-server/static
STATIC_FILES_DIR = PROJECT_ROOT / "static"


@asynccontextmanager
async def lifespan(app: FastAPI):
    print("Connecting to audio backend...")
    app.state.backend_client = await AudioBackendClient.connect("127.0.0.1", 9000)
    print("Connected!")
    yield

    print("Closing connection to audio backend...")
    await app.state.backend_client.close()
    print("Connection has being closed!")


app = FastAPI(title="Home media server. Not powered by Rust.", lifespan=lifespan)
app.include_router(healthz.router, prefix="/api")
app.include_router(ws_stream.router, prefix="/api")
app.mount("/", StaticFiles(directory=STATIC_FILES_DIR), name="static")


# from __future__ import annotations

# import asyncio

# from app.streaming.audio_backend import AudioBackendClient, AudioBackendError


# async def main():
#    async with await AudioBackendClient.connect("127.0.0.1", 9000) as client:
#        print("Connected to the Rust backend!")
#        new_port = await client.issue_new_worker()
#        print(f"Issued new worker on the {new_port} port")
#
#        closed_port = await client.drop_worker(new_port)
#        print(f"Colsed the worker on the {closed_port}!")


# if __name__ == "__main__":
#    asyncio.run(main())
