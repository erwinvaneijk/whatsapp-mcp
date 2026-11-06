"""Read settings from the shared YAML configuration file.

The bridge and the MCP server can read their settings from one file instead
of environment variables, which is easier for long lists such as the send
allowlist. The file is ~/.config/whatsapp-mcp/config.yaml, or the path in
WHATSAPP_MCP_CONFIG. An environment variable that is set takes precedence over
the same setting in the file.

whatsapp-bridge/config.go reads the same file; keep both in sync.
"""

import os
import stat
import sys
from pathlib import Path

import yaml

CONFIG_PATH_ENV = "WHATSAPP_MCP_CONFIG"

# Each setting in the file and its environment variable.
SETTINGS = {
    "bridge_token": "WHATSAPP_BRIDGE_TOKEN",
    "media_dir": "WHATSAPP_MEDIA_DIR",
    "log_content": "WHATSAPP_LOG_CONTENT",  # used by the bridge
    "pair_phone": "WHATSAPP_PAIR_PHONE",  # used by the bridge
    "send_allowlist": "WHATSAPP_SEND_ALLOWLIST",  # used by the bridge
    "read_only": "WHATSAPP_MCP_READ_ONLY",
}
# Settings that may be a YAML list; entries are joined with ", ".
LIST_SETTINGS = {"send_allowlist"}

_NULL = "tag:yaml.org,2002:null"


class ConfigError(Exception):
    pass


def flag(value: str) -> bool:
    """Whether a yes/no value means yes: 1, true, yes or on, in any case and
    with spaces around it. Anything else, including an empty value, is no."""
    return value.strip().lower() in ("1", "true", "yes", "on")


def config_path() -> "tuple[Path | None, bool]":
    """The file to read, and whether it must exist (only when WHATSAPP_MCP_CONFIG names it)."""
    explicit = os.environ.get(CONFIG_PATH_ENV, "").strip()
    if explicit:
        return Path(explicit).expanduser(), True
    try:
        return Path.home() / ".config" / "whatsapp-mcp" / "config.yaml", False
    except RuntimeError:
        return None, False


def parse_config(text: str) -> dict[str, str]:
    """Turn the YAML text into environment-variable values.

    Values keep their exact text (YAML would read 0612 as another number).
    """
    try:
        root = yaml.compose(text, Loader=yaml.SafeLoader)
    except yaml.YAMLError as e:
        raise ConfigError(str(e)) from e
    values: dict[str, str] = {}
    if root is None:
        return values  # empty file
    if not isinstance(root, yaml.MappingNode):
        raise ConfigError("the file must contain settings as `name: value` lines")
    for key_node, value in root.value:
        key, line = key_node.value, key_node.start_mark.line + 1
        env = SETTINGS.get(key)
        if env is None:
            raise ConfigError(f"line {line}: unknown setting {key!r}")
        if isinstance(value, yaml.ScalarNode):
            if value.tag != _NULL:
                values[env] = value.value
        elif isinstance(value, yaml.SequenceNode) and key in LIST_SETTINGS:
            items = []
            for item in value.value:
                if not isinstance(item, yaml.ScalarNode):
                    raise ConfigError(
                        f"line {item.start_mark.line + 1}: {key} entries must be single values"
                    )
                items.append(item.value)
            values[env] = ", ".join(items)
        else:
            raise ConfigError(f"line {value.start_mark.line + 1}: {key} must be a single value")
    return values


def load_config() -> Path | None:
    """Apply the configuration file to os.environ, for settings whose
    environment variable is not set. Returns the file used, or None."""
    path, required = config_path()
    if path is None:
        return None
    try:
        info = path.stat()
    except FileNotFoundError:
        if required:
            raise ConfigError(f"configuration file {path} does not exist") from None
        return None
    # The file holds the bridge token: refuse it when others can read or
    # change it. Windows permissions work differently; skip there.
    if os.name != "nt" and stat.S_IMODE(info.st_mode) & 0o077:
        raise ConfigError(f"{path} can be read or changed by other users; run: chmod 600 {path}")
    try:
        values = parse_config(path.read_text(encoding="utf-8"))
    except ConfigError as e:
        raise ConfigError(f"{path}: {e}") from e
    for env, value in values.items():
        if not os.environ.get(env):
            os.environ[env] = value
    return path


def load_config_or_exit() -> Path | None:
    """load_config, but stop with a clear message (on stderr) on any problem."""
    try:
        return load_config()
    except (ConfigError, OSError) as e:
        print(f"whatsapp-mcp: {e}", file=sys.stderr)
        sys.exit(1)
