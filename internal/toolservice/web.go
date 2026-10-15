package toolservice

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/core"
	"golang.org/x/net/html"
)

var specialNetworks = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"), netip.MustParsePrefix("100.64.0.0/10"),
	netip.MustParsePrefix("192.0.0.0/24"), netip.MustParsePrefix("192.0.2.0/24"),
	netip.MustParsePrefix("198.18.0.0/15"), netip.MustParsePrefix("198.51.100.0/24"),
	netip.MustParsePrefix("203.0.113.0/24"), netip.MustParsePrefix("240.0.0.0/4"),
	netip.MustParsePrefix("2001:db8::/32"), netip.MustParsePrefix("2002::/16"),
}

func publicIP(ip netip.Addr) bool {
	ip = ip.Unmap()
	if !ip.IsGlobalUnicast() || ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
		return false
	}
	if ip.Is6() && !netip.MustParsePrefix("2000::/3").Contains(ip) {
		return false
	}
	for _, prefix := range specialNetworks {
		if prefix.Contains(ip) {
			return false
		}
	}
	return true
}

func fetchURL(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Hostname() == "" || u.User != nil {
		return nil, errors.New("only public HTTP(S) URLs are supported")
	}
	if u.Port() != "" && u.Port() != "80" && u.Port() != "443" {
		return nil, errors.New("only ports 80 and 443 are supported")
	}
	return u, nil
}

func publicClient() *http.Client {
	return &http.Client{Timeout: 15 * time.Second, Transport: &http.Transport{
		MaxIdleConns: 64, MaxIdleConnsPerHost: 8, IdleConnTimeout: 30 * time.Second, ResponseHeaderTimeout: 10 * time.Second,
		DialContext: func(ctx context.Context, network, address string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(address)
			if err != nil {
				return nil, err
			}
			ips, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
			if err != nil || len(ips) == 0 {
				return nil, errors.New("DNS lookup failed")
			}
			for _, ip := range ips {
				if !publicIP(ip) {
					return nil, errors.New("private and special-use networks are blocked")
				}
			}
			// Dial the vetted address directly; DNS cannot change between check and use.
			return (&net.Dialer{Timeout: 10 * time.Second}).DialContext(ctx, network, net.JoinHostPort(ips[0].String(), port))
		},
	}, CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if len(via) >= 4 {
			return errors.New("too many redirects")
		}
		_, err := fetchURL(req.URL.String())
		return err
	}}
}

func (s *Service) webFetch(ctx context.Context, raw string) core.Result {
	u, err := fetchURL(raw)
	if err != nil {
		return core.ToolError(err.Error(), false)
	}
	req, _ := http.NewRequestWithContext(ctx, "GET", u.String(), nil)
	req.Header.Set("User-Agent", "Brain-web-fetch")
	res, err := s.fetch.Do(req)
	if err != nil {
		return core.ToolError("public page fetch failed or destination blocked", false)
	}
	defer res.Body.Close()
	if res.StatusCode < 200 || res.StatusCode >= 300 {
		return core.ToolError("page returned an unsuccessful HTTP status", false)
	}
	data, err := io.ReadAll(io.LimitReader(res.Body, (2<<20)+1))
	if err != nil || len(data) > 2<<20 {
		return core.ToolError("page exceeds 2 MiB or could not be read", false)
	}
	contentType := res.Header.Get("Content-Type")
	text := string(data)
	if strings.Contains(contentType, "text/html") {
		text = htmlText(text)
	} else if !strings.HasPrefix(contentType, "text/") && !strings.HasPrefix(contentType, "application/json") {
		return core.ToolError("page is not text, HTML, or JSON", false)
	}
	truncated := len(text) > 128<<10
	if truncated {
		text = strings.ToValidUTF8(text[:128<<10], "")
	}
	return textResult(map[string]any{"url": res.Request.URL.String(), "text": text, "truncated": truncated})
}

func htmlText(source string) string {
	tokenizer := html.NewTokenizer(strings.NewReader(source))
	var b strings.Builder
	ignored := ""
	for {
		switch tokenizer.Next() {
		case html.ErrorToken:
			return strings.Join(strings.Fields(b.String()), " ")
		case html.StartTagToken:
			name, _ := tokenizer.TagName()
			tag := string(name)
			if tag == "script" || tag == "style" || tag == "noscript" {
				ignored = tag
			}
		case html.EndTagToken:
			name, _ := tokenizer.TagName()
			if string(name) == ignored {
				ignored = ""
			}
			b.WriteByte(' ')
		case html.TextToken:
			if ignored == "" {
				b.Write(tokenizer.Text())
				b.WriteByte(' ')
			}
		}
	}
}

func (s *Service) webSearch(ctx context.Context, query string) core.Result {
	var req *http.Request
	if s.cfg.SearchProvider == "brave" {
		u := "https://api.search.brave.com/res/v1/web/search?q=" + url.QueryEscape(query) + "&count=10"
		req, _ = http.NewRequestWithContext(ctx, "GET", u, nil)
		req.Header.Set("X-Subscription-Token", s.cfg.SearchKey)
	} else if s.cfg.SearchProvider == "tavily" {
		data, _ := json.Marshal(map[string]any{"query": query, "max_results": 10})
		req, _ = http.NewRequestWithContext(ctx, "POST", "https://api.tavily.com/search", strings.NewReader(string(data)))
		req.Header.Set("Authorization", "Bearer "+s.cfg.SearchKey)
		req.Header.Set("Content-Type", "application/json")
	} else {
		return core.ToolError("search is not configured", false)
	}
	res, err := s.client.Do(req)
	if err != nil {
		return core.ToolError("search provider unavailable", false)
	}
	defer res.Body.Close()
	if res.StatusCode != 200 {
		return core.ToolError("search provider rejected the request", false)
	}
	data, err := io.ReadAll(io.LimitReader(res.Body, (512<<10)+1))
	if err != nil || len(data) > 512<<10 {
		return core.ToolError("search response exceeds limit", false)
	}
	type item struct {
		Title       string `json:"title"`
		URL         string `json:"url"`
		Content     string `json:"content,omitempty"`
		Description string `json:"description,omitempty"`
	}
	var response struct {
		Results []item `json:"results"`
		Web     struct {
			Results []item `json:"results"`
		} `json:"web"`
	}
	if json.Unmarshal(data, &response) != nil {
		return core.ToolError("invalid search response", false)
	}
	items := response.Results
	if s.cfg.SearchProvider == "brave" {
		items = response.Web.Results
	}
	if len(items) > 10 {
		items = items[:10]
	}
	return textResult(map[string]any{"results": items})
}
