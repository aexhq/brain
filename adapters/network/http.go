package network

import (
	"compress/gzip"
	"context"
	"crypto/tls"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

var forbidden = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"), netip.MustParsePrefix("100.64.0.0/10"),
	netip.MustParsePrefix("192.0.0.0/24"), netip.MustParsePrefix("192.0.2.0/24"),
	netip.MustParsePrefix("198.18.0.0/15"), netip.MustParsePrefix("198.51.100.0/24"),
	netip.MustParsePrefix("203.0.113.0/24"), netip.MustParsePrefix("240.0.0.0/4"),
	netip.MustParsePrefix("64:ff9b::/96"), netip.MustParsePrefix("2001::/32"),
	netip.MustParsePrefix("2001:db8::/32"), netip.MustParsePrefix("2002::/16"),
}

func Public(address netip.Addr) bool {
	address = address.Unmap()
	if !address.IsValid() || !address.IsGlobalUnicast() || address.IsPrivate() || address.IsLoopback() || address.IsLinkLocalUnicast() {
		return false
	}
	for _, prefix := range forbidden {
		if prefix.Contains(address) {
			return false
		}
	}
	return true
}

func ValidateURL(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.User != nil || u.Hostname() == "" || (u.Scheme != "http" && u.Scheme != "https") {
		return nil, kernel.Fail(wire.ErrorCodeForbidden, "network destination must be public HTTP or HTTPS without userinfo")
	}
	if port := u.Port(); port != "" && !((u.Scheme == "http" && port == "80") || (u.Scheme == "https" && port == "443")) {
		return nil, kernel.Fail(wire.ErrorCodeForbidden, "network destination port is not admitted")
	}
	if ip, err := netip.ParseAddr(u.Hostname()); err == nil && !Public(ip) {
		return nil, kernel.Fail(wire.ErrorCodeForbidden, "private or reserved destination is denied")
	}
	return u, nil
}

func Client(timeout time.Duration) *http.Client {
	dialer := net.Dialer{Timeout: 10 * time.Second, KeepAlive: -1}
	transport := &http.Transport{DisableKeepAlives: true, DisableCompression: true, MaxResponseHeaderBytes: 32768,
		TLSHandshakeTimeout: 10 * time.Second, ResponseHeaderTimeout: timeout, TLSClientConfig: &tls.Config{MinVersion: tls.VersionTLS12},
		DialContext: func(ctx context.Context, network, address string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(address)
			if err != nil {
				return nil, err
			}
			addresses, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
			if err != nil {
				return nil, err
			}
			if len(addresses) == 0 || len(addresses) > 32 {
				return nil, kernel.Fail(wire.ErrorCodeForbidden, "DNS answer set is empty or exceeds its bound")
			}
			for _, ip := range addresses {
				if !Public(ip) {
					return nil, kernel.Fail(wire.ErrorCodeForbidden, "DNS resolved to a private or reserved destination")
				}
			}
			// Dial the validated address, not the hostname: a second DNS lookup
			// would introduce a rebinding window.
			return dialer.DialContext(ctx, network, net.JoinHostPort(addresses[0].String(), port))
		}}
	return &http.Client{Transport: transport, Timeout: timeout, CheckRedirect: func(request *http.Request, via []*http.Request) error {
		if len(via) >= 5 {
			return kernel.Fail(wire.ErrorCodeInvalid, "redirect limit exceeded")
		}
		if _, err := ValidateURL(request.URL.String()); err != nil {
			return err
		}
		if len(via) > 0 && (request.URL.Host != via[0].URL.Host || request.URL.Scheme != via[0].URL.Scheme) {
			request.Header.Del("Authorization")
			request.Header.Del("Cookie")
			request.Header.Del("X-Api-Key")
			request.Header.Del("X-Subscription-Token")
		}
		return nil
	}}
}

func Body(response *http.Response, limit int64) ([]byte, error) {
	defer response.Body.Close()
	reader := io.Reader(io.LimitReader(response.Body, limit+1))
	encoding := strings.TrimSpace(strings.ToLower(response.Header.Get("Content-Encoding")))
	if encoding == "gzip" {
		gzipReader, err := gzip.NewReader(reader)
		if err != nil {
			return nil, err
		}
		defer gzipReader.Close()
		reader = gzipReader
	} else if encoding != "" && encoding != "identity" {
		return nil, fmt.Errorf("unsupported content encoding")
	}
	body, err := io.ReadAll(io.LimitReader(reader, limit+1))
	if int64(len(body)) > limit {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "response exceeds the compressed or decoded byte limit")
	}
	return body, err
}
