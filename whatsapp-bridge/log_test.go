package main

import "testing"

func TestEnvFlag(t *testing.T) {
	for _, v := range []string{"1", "true", "TRUE", " yes ", "on"} {
		if !envFlag(v) {
			t.Errorf("envFlag(%q) = false, want true", v)
		}
	}
	for _, v := range []string{"", "0", "false", "no", "off", "2", "enabled"} {
		if envFlag(v) {
			t.Errorf("envFlag(%q) = true, want false", v)
		}
	}
}

func TestResultLogLine(t *testing.T) {
	// Without WHATSAPP_LOG_CONTENT the status text (emoji, file paths) is left out.
	if got := resultLogLine("Reaction sent", true, "Reaction 👍 sent", false); got != "Reaction sent true" {
		t.Errorf("success without content: %q", got)
	}
	got := resultLogLine("Message sent", false, "Error reading media file: open /home/me/outbox/salary.pdf", false)
	if got != "Message sent false (set WHATSAPP_LOG_CONTENT=1 to log the error)" {
		t.Errorf("failure without content: %q", got)
	}
	// With it, the full status is logged.
	if got := resultLogLine("Reaction sent", true, "Reaction 👍 sent", true); got != "Reaction sent true Reaction 👍 sent" {
		t.Errorf("with content: %q", got)
	}
}
