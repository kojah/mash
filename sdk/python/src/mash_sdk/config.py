"""The `[[destinations]]` tables of connectors' strict TOML configuration."""

from pathlib import Path
from typing import Annotated, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, StrictStr

from . import destination


def absolute(value: Path, base: Path) -> Path:
    """`~` expands; relative paths are taken from `base`, the config's directory."""
    value = value.expanduser()
    return (value if value.is_absolute() else base / value).resolve()


class Local(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)
    type: Literal["local"]
    database: Path
    executable: StrictStr = "mash"
    config: Path | None = None

    def relative_to(self, base: Path) -> Self:
        config = None if self.config is None else absolute(self.config, base)
        return self.model_copy(
            update={"database": absolute(self.database, base), "config": config}
        )

    def destination(self) -> destination.Destination:
        return destination.Destination.local(
            database=self.database, executable=self.executable, config=self.config
        )


class Shared(BaseModel):
    """Credentials stay in the `token_env` environment variable."""

    model_config = ConfigDict(extra="forbid", frozen=True)
    type: Literal["shared"]
    api_url: StrictStr
    group_id: StrictStr
    node_id: StrictStr
    token_env: StrictStr

    def relative_to(self, base: Path) -> Self:
        return self

    def destination(self) -> destination.Destination:
        return destination.Destination.shared(
            api_url=self.api_url,
            group_id=self.group_id,
            node_id=self.node_id,
            token_env=self.token_env,
        )


DestinationConfig = Annotated[Local | Shared, Field(discriminator="type")]
