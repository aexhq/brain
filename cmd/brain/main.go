package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"github.com/aexhq/brain"
	"log/slog"
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
	if *check {
		if err := brain.Validate(opts); err != nil {
			return err
		}
		fmt.Println("configuration valid")
		return nil
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	server, err := brain.New(ctx, opts)
	if err != nil {
		return err
	}
	return server.Run(ctx)
}
