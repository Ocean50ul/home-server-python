import pytest
from fastapi.testclient import TestClient
from fastapi import status
from app.main import app
from app.api.healthz import HealthyResponse


@pytest.fixture(scope="module")
def client() -> TestClient:
    """
    Creates a TestClient instance for the tests.
    """
    return TestClient(app)


def test_read_healthz(client: TestClient):
    response = client.get("/healthz")
    assert response.status_code == status.HTTP_200_OK

    payload = HealthyResponse.model_validate_json(response.text)
    assert payload == HealthyResponse()
