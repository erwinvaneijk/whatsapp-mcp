package main

import (
	"encoding/json"
	"os"
	"reflect"
	"sort"
	"strings"
	"testing"
)

// The configuration cases shared with the Python server and the Rust config
// crate (parity/config-cases.json): all three must follow the same rules.
type sharedConfigCases struct {
	Files []struct {
		Name          string            `json:"name"`
		YAML          string            `json:"yaml"`
		Values        map[string]string `json:"values"`
		ErrorContains []string          `json:"error_contains"`
	} `json:"files"`
	Allowlists []struct {
		Value string   `json:"value"`
		JIDs  []string `json:"jids"`
		Error bool     `json:"error"`
	} `json:"allowlists"`
	Flags struct {
		On  []string `json:"on"`
		Off []string `json:"off"`
	} `json:"flags"`
}

func loadSharedConfigCases(t *testing.T) sharedConfigCases {
	t.Helper()
	data, err := os.ReadFile("../parity/config-cases.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases sharedConfigCases
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	return cases
}

func TestSharedConfigFiles(t *testing.T) {
	for _, c := range loadSharedConfigCases(t).Files {
		got, err := parseConfig([]byte(c.YAML))
		if c.ErrorContains != nil {
			if err == nil {
				t.Errorf("%s: want an error, got %v", c.Name, got)
				continue
			}
			for _, fragment := range c.ErrorContains {
				if !strings.Contains(err.Error(), fragment) {
					t.Errorf("%s: error %q should contain %q", c.Name, err, fragment)
				}
			}
			continue
		}
		if err != nil {
			t.Errorf("%s: unexpected error: %v", c.Name, err)
			continue
		}
		if !reflect.DeepEqual(got, c.Values) {
			t.Errorf("%s: got %v, want %v", c.Name, got, c.Values)
		}
	}
}

func TestSharedConfigAllowlists(t *testing.T) {
	for _, c := range loadSharedConfigCases(t).Allowlists {
		a, err := parseSendAllowlist(c.Value)
		if c.Error {
			if err == nil {
				t.Errorf("%q: want an error", c.Value)
			}
			continue
		}
		if err != nil {
			t.Errorf("%q: unexpected error: %v", c.Value, err)
			continue
		}
		var got []string
		if a != nil {
			for jid := range a.jids {
				got = append(got, jid)
			}
			sort.Strings(got)
		}
		if !reflect.DeepEqual(got, c.JIDs) {
			t.Errorf("%q: got %v, want %v", c.Value, got, c.JIDs)
		}
	}
}

func TestSharedConfigFlags(t *testing.T) {
	flags := loadSharedConfigCases(t).Flags
	for _, v := range flags.On {
		if !envFlag(v) {
			t.Errorf("%q must mean yes", v)
		}
	}
	for _, v := range flags.Off {
		if envFlag(v) {
			t.Errorf("%q must mean no", v)
		}
	}
}
