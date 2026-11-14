// Package api defines generic isolated environments, files and process streams.
package api

type Create struct {
	Tenant  string `json:"tenant"`
	Session string `json:"session"`
	Profile string `json:"profile"`
}
type Environment struct {
	ID      string `json:"id"`
	Profile string `json:"profile"`
}
type Process struct {
	Command []string `json:"command"`
}
type Failure struct {
	Error string `json:"error"`
}
