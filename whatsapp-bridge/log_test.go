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
