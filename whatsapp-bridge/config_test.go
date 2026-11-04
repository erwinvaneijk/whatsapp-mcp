package main

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

const exampleConfig = `
# Settings for the bridge and the MCP server
bridge_token: "0123456789abcdef0123456789abcdef"
media_dir: /home/me/whatsapp-outbox
log_content: no
pair_phone: 0612345678        # kept as written, not read as a number
read_only:                    # empty: not set
send_allowlist:
  - 31612345678               # Sien
  - "+31 6 8765 4321"         # Luc
  - 120363000000000000@g.us   # Huis
`

func TestParseConfig(t *testing.T) {
	got, err := parseConfig([]byte(exampleConfig))
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]string{
		"WHATSAPP_BRIDGE_TOKEN":   "0123456789abcdef0123456789abcdef",
		"WHATSAPP_MEDIA_DIR":      "/home/me/whatsapp-outbox",
		"WHATSAPP_LOG_CONTENT":    "no",
		"WHATSAPP_PAIR_PHONE":     "0612345678",
		"WHATSAPP_SEND_ALLOWLIST": "31612345678, +31 6 8765 4321, 120363000000000000@g.us",
	}
	if len(got) != len(want) {
		t.Errorf("got %d settings, want %d: %v", len(got), len(want), got)
	}
	for k, v := range want {
		if got[k] != v {
			t.Errorf("%s = %q, want %q", k, got[k], v)
		}
	}
	// The list from the file must be accepted by the allowlist parser.
	if a, err := parseSendAllowlist(got["WHATSAPP_SEND_ALLOWLIST"]); err != nil || len(a.jids) != 3 {
		t.Errorf("allowlist from the file: %v, %v", a, err)
	}
	// A single line works too.
	if got, _ := parseConfig([]byte("send_allowlist: 316123, 316456")); got["WHATSAPP_SEND_ALLOWLIST"] != "316123, 316456" {
		t.Errorf("one-line allowlist: %q", got["WHATSAPP_SEND_ALLOWLIST"])
	}
	if got, err := parseConfig([]byte("# only comments\n")); err != nil || len(got) != 0 {
		t.Errorf("empty file: %v, %v", got, err)
	}
}

func TestParseConfigErrors(t *testing.T) {
	for name, text := range map[string]string{
		"typo in a name":        "bridge_tokn: x",
		"list for a value":      "media_dir: [a, b]",
		"nested entry in list":  "send_allowlist:\n  - a: b",
		"not a list of setting": "- just a list",
		"invalid YAML":          "bridge_token: [unclosed",
	} {
		if _, err := parseConfig([]byte(text)); err == nil {
			t.Errorf("%s: want an error", name)
		}
	}
	_, err := parseConfig([]byte("log_content: yes\nsend_alowlist: 1"))
	if err == nil || !strings.Contains(err.Error(), "line 2") || !strings.Contains(err.Error(), "send_alowlist") {
		t.Errorf("error should name the line and the setting: %v", err)
	}
}

func writeConfig(t *testing.T, mode os.FileMode, text string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "config.yaml")
	if err := os.WriteFile(path, []byte(text), mode); err != nil {
		t.Fatal(err)
	}
	os.Chmod(path, mode) // WriteFile applies the umask
	return path
}

func TestLoadConfig(t *testing.T) {
	path := writeConfig(t, 0o600, "media_dir: /from/file\npair_phone: 31600000000\n")
	t.Setenv(configPathEnv, path)
	t.Setenv("WHATSAPP_MEDIA_DIR", "/from/env") // set: wins over the file
	t.Setenv("WHATSAPP_PAIR_PHONE", "")         // empty: the file fills it in

	used, err := loadConfig()
	if err != nil || used != path {
		t.Fatalf("loadConfig: %q, %v", used, err)
	}
	if v := os.Getenv("WHATSAPP_MEDIA_DIR"); v != "/from/env" {
		t.Errorf("environment must take precedence, got %q", v)
	}
	if v := os.Getenv("WHATSAPP_PAIR_PHONE"); v != "31600000000" {
		t.Errorf("unset setting must come from the file, got %q", v)
	}
}

func TestLoadConfigMissingFile(t *testing.T) {
	// The default file is optional.
	t.Setenv(configPathEnv, "")
	t.Setenv("HOME", t.TempDir())
	if used, err := loadConfig(); used != "" || err != nil {
		t.Errorf("no default file: %q, %v", used, err)
	}
	// A file named in WHATSAPP_CONFIG must exist.
	t.Setenv(configPathEnv, filepath.Join(t.TempDir(), "missing.yaml"))
	if _, err := loadConfig(); err == nil {
		t.Error("missing WHATSAPP_CONFIG file: want an error")
	}
}

func TestLoadConfigRefusesReadableFile(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows permissions work differently")
	}
	t.Setenv(configPathEnv, writeConfig(t, 0o644, "bridge_token: secret\n"))
	_, err := loadConfig()
	if err == nil || !strings.Contains(err.Error(), "chmod 600") {
		t.Errorf("a file others can read must be refused: %v", err)
	}
}
