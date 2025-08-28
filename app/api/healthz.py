from fastapi import APIRouter

router = APIRouter()


@router.get("/healthz")
def read_healthz():
    """
    Still sane, Exile?
    """
    return {"status": "ok", "version": "0.0.1"}
