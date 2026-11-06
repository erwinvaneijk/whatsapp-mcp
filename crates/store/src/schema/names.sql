-- Contact names and the LID-to-phone map, written by the Rust bridge
-- (whatsapp-mcp bridge). The MCP server reads names from here; with the Go
-- bridge it reads whatsmeow's tables in whatsapp.db instead. Same columns and
-- meaning as whatsmeow_contacts and whatsmeow_lid_map.

CREATE TABLE IF NOT EXISTS contacts (
	jid           TEXT PRIMARY KEY,
	first_name    TEXT,
	full_name     TEXT,
	push_name     TEXT,
	business_name TEXT
);

CREATE TABLE IF NOT EXISTS lid_map (
	lid TEXT PRIMARY KEY,
	pn  TEXT UNIQUE NOT NULL
);
