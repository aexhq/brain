# Stored session credentials

This Go package encrypts credential records with AES-256-GCM and binds ciphertext
to a tenant, session and credential binding. It contains code, not secret files.
Configured keys are loaded from environment variables. Storage persists opaque
records; stored data and backups alone do not disclose the credential values.

Brain encrypts admitted model and provider credentials. The Tools service opens
the selected provider credential when resolving its authorized binding. Trusted
service configuration supplies compatible keys. Provider code receives that
binding's credential; tool authors do not implement encryption. Plaintext
credentials stay outside model arguments, session configuration and transcripts.

A binding combines resource parameters, selected operations and authorization;
possessing a credential does not grant permission to every provider operation.

Moving provider credential ownership fully into Tools, behind its private
encrypted store, remains a design proposal in the implementation plan. There is
no general Brain decrypt endpoint or tool-facing secret lookup API.
