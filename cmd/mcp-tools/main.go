// mcp-tools discovers selected remote tools and prints static Brain bindings.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/tools"
)

func main() {
	endpoint := flag.String("url", "", "MCP Streamable HTTP endpoint")
	token := flag.String("token-env", "", "environment variable containing bearer token")
	tenant := flag.String("tenant", "", "authorized tenant")
	names := flag.String("tools", "", "comma-separated remote tool names")
	flag.Parse()
	if *tenant == "" || *names == "" || flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "tenant and tools are required")
		os.Exit(1)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	bindings, err := tools.DiscoverMCP(ctx, tools.Binding{URL: *endpoint, TokenEnv: *token, Tenants: []string{*tenant}}, strings.Split(*names, ","))
	if err != nil {
		fmt.Fprintln(os.Stderr, "MCP discovery failed:", err)
		os.Exit(1)
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(bindings); err != nil {
		os.Exit(1)
	}
}
