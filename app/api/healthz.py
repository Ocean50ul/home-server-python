from typing import Literal
from fastapi import APIRouter
from pydantic import BaseModel

router = APIRouter()


class HealthyResponse(BaseModel):
    status: Literal["ok"] = "ok"
    version: str = "0.0.1"


@router.get("/healthz", response_model=HealthyResponse)
def read_healthz():
    """
    Still sane, Exile?
    """
    return HealthyResponse()
