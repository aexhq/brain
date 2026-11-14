package brain

import (
	"context"
	"net/http"
	"strings"
	"time"
)

func dependenciesReady(urls ...string) func(context.Context) bool {
	client := &http.Client{Timeout: 2 * time.Second}
	return func(ctx context.Context) bool {
		results := make(chan bool, len(urls))
		for _, url := range urls {
			go func() {
				r, err := http.NewRequestWithContext(ctx, "GET", strings.TrimRight(url, "/")+"/health/live", nil)
				if err != nil {
					results <- false
					return
				}
				response, err := client.Do(r)
				if err != nil {
					results <- false
					return
				}
				response.Body.Close()
				results <- response.StatusCode == 200
			}()
		}
		ready := true
		for range urls {
			ready = <-results && ready
		}
		return ready
	}
}
