package main

import (
	"os"
	"testing"
)

// config.example.yaml must stay valid and match the settings the code knows.
func TestExampleConfigParses(t *testing.T) {
	data, err := os.ReadFile("../config.example.yaml")
	if err != nil {
		t.Fatal(err)
	}
	got, err := parseConfig(data)
	if err != nil {
		t.Fatal(err)
	}
	// The example token is empty, so a copied file cannot run with a known secret.
	if got["WHATSAPP_BRIDGE_TOKEN"] != "" || got["WHATSAPP_SEND_ALLOWLIST"] != "" || got["WHATSAPP_MCP_READ_ONLY"] != "no" {
		t.Errorf("unexpected: %v", got)
	}
	if _, err := parseSendAllowlist(got["WHATSAPP_SEND_ALLOWLIST"]); err != nil {
		t.Error(err)
	}
}
