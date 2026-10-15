package models

import (
	"io"
	"net"
	"net/http"
	"time"
)

func transport() *http.Transport {
	protocols := new(http.Protocols)
	protocols.SetHTTP1(true)
	return &http.Transport{
		Protocols: protocols, DisableKeepAlives: true, DisableCompression: true,
		MaxResponseHeaderBytes: 32768, TLSHandshakeTimeout: 10 * time.Second,
		ResponseHeaderTimeout: 300 * time.Second,
		DialContext:           (&net.Dialer{Timeout: 10 * time.Second, KeepAlive: -1}).DialContext,
	}
}

type boundedTransport struct {
	base  http.RoundTripper
	limit int64
}

type boundedBody struct {
	io.Reader
	io.Closer
}

func (t boundedTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	request = request.Clone(request.Context())
	request.GetBody = nil
	request.Close = true
	response, err := t.base.RoundTrip(request)
	if response != nil {
		response.Body = boundedBody{io.LimitReader(response.Body, t.limit+1), response.Body}
	}
	return response, err
}
