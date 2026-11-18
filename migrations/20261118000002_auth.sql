-- Authentication (ADR-0004): one row per account, its password only ever in
-- credentials, its OIDC doors only ever in identities. Usernames and emails
-- are stored lowercased by the handlers; NOCASE uniques keep the index
-- honest even if a future caller forgets.
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL
);

CREATE TABLE credentials (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    password_hash TEXT NOT NULL
);

CREATE TABLE identities (
    provider_name TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    PRIMARY KEY (issuer, subject)
);
