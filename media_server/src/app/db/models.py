from sqlalchemy import Column, Integer, String
from sqlalchemy.orm import DeclarativeBase


class Base(DeclarativeBase):
    pass


class HelloWorldSqlite(Base):
    __tablename__ = "helloworld"

    id = Column(Integer, primary_key=True)
    name = Column(String(50), nullable=False)
    email = Column(String(100), nullable=False)

    def __repr__(self) -> str:
        return f"User(id={self.id}, name='{self.name}', email={self.email})"
