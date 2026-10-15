package main

import (
	"context"
	"encoding/json"
	"flag"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/aexhq/brain/internal/toolservice"
)

func main() {
	listen := flag.String("listen", "127.0.0.1:8092", "HTTP address")
	catalog := flag.String("catalog", "", "print descriptors using this service base URL")
	tenant := flag.String("tenant", "local", "tenant for generated descriptors")
	flag.Parse()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	service, err := toolservice.New(ctx, toolservice.FromEnv())
	if err != nil {
		log.Fatal(err)
	}
	if *catalog != "" {
		e := json.NewEncoder(os.Stdout)
		e.SetIndent("", "  ")
		if err := e.Encode(service.Catalog(*catalog, *tenant)); err != nil {
			log.Fatal(err)
		}
		return
	}
	server := &http.Server{Addr: *listen, Handler: service.Handler(), ReadHeaderTimeout: 10 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second}
	go func() {
		<-ctx.Done()
		drain, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = server.Shutdown(drain)
	}()
	log.Printf("resource tools listening on %s", *listen)
	if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatal(err)
	}
}
