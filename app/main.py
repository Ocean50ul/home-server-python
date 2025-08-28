from fastapi import FastAPI
from app.api import healthz

app = FastAPI(title="Home media server. Not powered by Rust.")
app.include_router(healthz.router)
