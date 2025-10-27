import asyncio
from asyncio.tasks import Task
import logging

from fastapi import WebSocket

from app.streaming import AudioBackendClient, AudioBackendError as BaseAudioBackendError
from app.streaming.audio_backend.client import (
    BackendDisconnectedError,
    InternalBackendError,
)
from app.streaming.client_session.session import WorkerDisconnectedError

from app.streaming.client_session.session import (
    Session,
    SessionError as BaseSessionError,
)

logger = logging.getLogger(__name__)


class SessionsManagerError(Exception):
    """Base exception for SessionsManager"""

    pass


class AudioBackendError(SessionsManagerError):
    """Audio backend related error"""

    pass


class SessionError(SessionsManagerError):
    """Session related error"""

    pass


class ConnectionError(SessionsManagerError):
    """Connection failed"""

    pass


class SessionsManager:
    _backend_client: AudioBackendClient
    _sessions: dict[int, Session]
    _max_sessions: int

    def __init__(
        self,
        backend_client: AudioBackendClient,
        max_sessions: int = 20,
    ) -> None:
        self._backend_client = backend_client
        self._sessions = {}
        self._max_sessions = max_sessions

    @classmethod
    async def create(
        cls, host: str, port: int, max_sessions: int = 20
    ) -> "SessionsManager":
        """
        Raise:
            SessionsManagerError.ConnectionError(AudiobackendError) on failure to connect to the backend
        """
        try:
            backend_client = await AudioBackendClient.connect(host, port)
            return cls(backend_client, max_sessions)
        except BaseAudioBackendError as e:
            raise ConnectionError(f"Failed to connect to {host}:{port}: {e}") from e

    async def _cleanup(self, worker_port: int):
        try:
            await self._backend_client.drop_worker(worker_port)
        except Exception as cleanup_error:
            logger.error(
                f"Failed to create a session and failed to cleanup worker {worker_port}: {cleanup_error}"
            )

    @property
    def _active(self) -> int:
        return len(self._sessions)

    async def create_session(self, websocket: WebSocket) -> bool:
        """
        Raises:
            ConnectionError: on failure to connect to the backend
            AudioBackendError: internal audio backend error on failure to drop the worker
            SessionError: on failure to start the session
        """
        if self._active >= self._max_sessions:
            return False

        try:
            # ================ FUNCTION BODY ======================
            worker_port = await self._backend_client.issue_new_worker()
            (worker_reader, worker_writer) = await asyncio.open_connection(
                "127.0.0.1", worker_port
            )

            session = Session(worker_reader, worker_writer, worker_port, websocket)
            await session.start()

            self._sessions[worker_port] = session
            return True
            # ================ FUNCTION BODY ======================

        except BackendDisconnectedError as e:
            raise ConnectionError(f"Failed to issue new worker: {e}") from e
        except InternalBackendError as e:
            raise AudioBackendError(f"Failed to issue new worker: {e}") from e
        except BaseSessionError as e:
            # ==================== CLEANUP ============================
            await self._cleanup(worker_port)
            # ==================== CLEANUP ============================

            raise SessionError(f"Failed to start the session: {e}") from e

    def _on_session_done_callback(self, task: Task):
        exc = task.exception()

        if isinstance(exc, WorkerDisconnectedError):
            logger.info(
                f"Session task on port {exc.port} has returned with an error: {exc}"
            )
            asyncio.create_task(self.stop_session(exc.port))

    async def stop_session(self, worker_port: int) -> bool:
        """
        Raises:
            SessionError: on failure to start the session
            AudioBackendError: internal audio backend error on failure to drop the worker
        """
        if session := self._sessions.pop(worker_port, None):
            try:
                await session.close()
                _ = await self._backend_client.drop_worker(worker_port)
                return True
            except BaseSessionError as e:
                # ==================== CLEANUP ============================
                await self._cleanup(worker_port)
                # ==================== CLEANUP ============================

                raise SessionError(f"Failed to stop the session: {e}") from e
            except InternalBackendError as e:
                raise AudioBackendError(f"Failed to drop the worker: {e}") from e

        return False

    async def close_all(self):
        logger.info("Closing all active sessions...")

        for port in list(self._sessions.keys()):
            try:
                await self.stop_session(port)
            except Exception:
                logger.error(
                    f"Close all error: failed to close the session on port {port}"
                )

        logger.info("Done.")
