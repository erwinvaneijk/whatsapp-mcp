package main

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"gopkg.in/yaml.v3"
)

// The bridge and the MCP server can read their settings from one YAML file
// instead of environment variables, which is easier for long lists such as
// the send allowlist. The file is ~/.config/whatsapp-mcp/config.yaml, or the
// path in WHATSAPP_MCP_CONFIG. An environment variable that is set takes
// precedence over the same setting in the file.
//
// The Rust config crate (crates/config) reads the same file; keep both in
// sync. parity/config-cases.json holds the cases both must pass.

const configPathEnv = "WHATSAPP_MCP_CONFIG"

// configSettings maps each setting in the file to its environment variable.
var configSettings = map[string]string{
	"bridge_token":   "WHATSAPP_BRIDGE_TOKEN",
	"media_dir":      "WHATSAPP_MEDIA_DIR",
	"log_content":    "WHATSAPP_LOG_CONTENT",
	"pair_phone":     "WHATSAPP_PAIR_PHONE",
	"send_allowlist": "WHATSAPP_SEND_ALLOWLIST",
	"read_only":      "WHATSAPP_MCP_READ_ONLY", // used by the MCP server
}

// listSettings may be a YAML list; its entries are joined with ", ".
var listSettings = map[string]bool{"send_allowlist": true}

// configPath returns the file to read and whether it must exist (only when
// WHATSAPP_MCP_CONFIG names it explicitly).
func configPath() (string, bool) {
	if p := strings.TrimSpace(os.Getenv(configPathEnv)); p != "" {
		return p, true
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", false
	}
	return filepath.Join(home, ".config", "whatsapp-mcp", "config.yaml"), false
}

// parseConfig reads the YAML text into environment-variable values. Values
// keep their exact text (YAML would turn 0612 or 1e3 into other numbers).
func parseConfig(data []byte) (map[string]string, error) {
	var doc yaml.Node
	if err := yaml.Unmarshal(data, &doc); err != nil {
		return nil, err
	}
	values := map[string]string{}
	if len(doc.Content) == 0 {
		return values, nil // empty file
	}
	root := doc.Content[0]
	if root.Kind != yaml.MappingNode {
		return nil, errors.New("the file must contain settings as `name: value` lines")
	}
	for i := 0; i+1 < len(root.Content); i += 2 {
		key, value := root.Content[i].Value, root.Content[i+1]
		env, known := configSettings[key]
		if !known {
			return nil, fmt.Errorf("line %d: unknown setting %q", root.Content[i].Line, key)
		}
		if value.Tag == "!!null" {
			continue
		}
		switch {
		case value.Kind == yaml.ScalarNode:
			values[env] = value.Value
		case value.Kind == yaml.SequenceNode && listSettings[key]:
			items := make([]string, 0, len(value.Content))
			for _, item := range value.Content {
				if item.Kind != yaml.ScalarNode {
					return nil, fmt.Errorf("line %d: %s entries must be single values", item.Line, key)
				}
				items = append(items, item.Value)
			}
			values[env] = strings.Join(items, ", ")
		default:
			return nil, fmt.Errorf("line %d: %s must be a single value", value.Line, key)
		}
	}
	return values, nil
}

// checkConfigPermissions refuses a file that others can read or change: it
// holds the bridge token. Windows permissions work differently; skip there.
func checkConfigPermissions(path string, info fs.FileInfo) error {
	if runtime.GOOS == "windows" {
		return nil
	}
	if info.Mode().Perm()&0o077 != 0 {
		return fmt.Errorf("%s can be read or changed by other users; run: chmod 600 %s", path, path)
	}
	return nil
}

// loadConfig applies the configuration file to the environment, for settings
// whose environment variable is not set. It returns the file used, or "".
func loadConfig() (string, error) {
	path, required := configPath()
	if path == "" {
		return "", nil
	}
	info, err := os.Stat(path)
	if errors.Is(err, fs.ErrNotExist) && !required {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("configuration file: %v", err)
	}
	if err := checkConfigPermissions(path, info); err != nil {
		return "", err
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("configuration file: %v", err)
	}
	values, err := parseConfig(data)
	if err != nil {
		return "", fmt.Errorf("%s: %v", path, err)
	}
	for env, value := range values {
		if os.Getenv(env) == "" {
			os.Setenv(env, value)
		}
	}
	return path, nil
}
