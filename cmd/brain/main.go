package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"github.com/aexhq/brain"
	"github.com/aexhq/brain/integrations"
	"io"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"syscall"
)

func main() {
	if err := run(); err != nil {
		slog.Error("brain stopped", "error", err)
		os.Exit(1)
	}
}
func run() error {
	file := flag.String("config", "brain.json", "host configuration JSON")
	check := flag.Bool("check", false, "validate configuration without serving")
	stdio := flag.Bool("stdio", false, "report the listening URL on stdout and stop when stdin closes")
	toolProviders := flag.String("tool-providers", "", "installed integration configuration JSON")
	flag.Parse()
	if flag.NArg() != 0 {
		return errors.New("unexpected arguments")
	}
	logger := slog.New(slog.NewJSONHandler(os.Stderr, nil))
	slog.SetDefault(logger)
	cfg, err := brain.LoadConfig(*file)
	if err != nil {
		return err
	}
	opts := brain.Options{Config: cfg, Logger: logger}
	if *toolProviders != "" {
		opts.Providers, err = integrations.LoadBuiltins(*toolProviders)
		if err != nil {
			return err
		}
	}
	if *check {
		if err := brain.Validate(opts); err != nil {
			return err
		}
		fmt.Println("configuration valid")
		return nil
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if *stdio {
		// The parent owns this process. EOF also shuts it down after a parent crash.
		go func() { _, _ = io.Copy(io.Discard, os.Stdin); stop() }()
		listener, err := net.Listen("tcp", cfg.Listen)
		if err != nil {
			return err
		}
		defer listener.Close()
		opts.Config.Listen = listener.Addr().String()
		opts.Config.PublicURL = "http://" + listener.Addr().String()
		server, err := brain.New(ctx, opts)
		if err != nil {
			return err
		}
		if err := json.NewEncoder(os.Stdout).Encode(map[string]string{"url": opts.Config.PublicURL}); err != nil {
			return errors.Join(err, server.Shutdown(context.Background()))
		}
		return server.Serve(ctx, listener)
	}
	server, err := brain.New(ctx, opts)
	if err != nil {
		return err
	}
	return server.Run(ctx)
}
