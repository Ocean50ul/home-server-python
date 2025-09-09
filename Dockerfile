FROM python:3.13-slim AS base
WORKDIR /code
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY ./app ./app
COPY ./alembic ./alembic
COPY alembic.ini alembic.ini

# non-root user (security)
RUN useradd -m -u 10001 appuser && chown -R appuser:appuser /code
USER appuser

# exposes the port
EXPOSE 8000

# -------- runtime image --------
FROM base AS runtime
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]

# -------- test image --------
FROM base AS test
USER root
COPY requirements-dev.txt .
COPY pyproject.toml .
RUN pip install --no-cache-dir -r requirements-dev.txt
COPY --chown=appuser:appuser ./tests ./tests
USER appuser
CMD ["pytest"]