package harness

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
)

// PoolLimits adds optional aggregate budgets to adapter-specific host limits.
// Zero means no additional limit; a configured ceiling rejects excess work.
type PoolLimits struct {
	Hosts       int `json:"hosts,omitempty"`
	Starting    int `json:"starting,omitempty"`
	ActiveTurns int `json:"active_turns,omitempty"`
}

func (l PoolLimits) Validate() error {
	if l.Hosts < 0 || l.Starting < 0 || l.ActiveTurns < 0 {
		return errors.New("invalid aggregate host limits")
	}
	return nil
}

// HostPool owns native hosts across installed process factories. Factories
// determine which sessions can share a host; the pool never mixes their state.
type HostPool struct {
	limits           PoolLimits
	mu               sync.Mutex
	groups           map[*ProcessFactory]*hostGroup
	starting, active int
	closed           bool
}

type hostGroup struct {
	hosts   []*process
	opening chan struct{}
}

func NewHostPool(limits PoolLimits) (*HostPool, error) {
	if err := limits.Validate(); err != nil {
		return nil, err
	}
	return &HostPool{limits: limits, groups: map[*ProcessFactory]*hostGroup{}}, nil
}

// UseHostPool installs application ownership before the factory first opens.
func (f *ProcessFactory) UseHostPool(pool *HostPool) error {
	if f.group != nil || (f.Pool != nil && f.Pool != pool) {
		return errors.New("process factory already has host ownership; install a fresh factory with the application pool")
	}
	f.Pool = pool
	return nil
}

func (f *ProcessFactory) initialize() {
	if f.Pool == nil {
		f.Pool, f.initErr = NewHostPool(PoolLimits{})
	}
	if f.MaxShared == 0 {
		f.MaxShared = 1
	}
	if f.MaxActiveTurns == 0 {
		f.MaxActiveTurns = f.MaxShared
	}
	if f.MaxHosts == 0 {
		f.MaxHosts = 64
	}
	if f.MaxStarting == 0 {
		f.MaxStarting = 4
	}
	if f.MaxShared < 1 || f.MaxShared > 1024 || f.MaxActiveTurns < 1 || f.MaxActiveTurns > f.MaxShared || f.MaxHosts < 1 || f.MaxStarting < 1 {
		f.initErr = errors.New("invalid harness host limits")
		return
	}
	f.Pool.mu.Lock()
	defer f.Pool.mu.Unlock()
	if f.Pool.groups == nil {
		f.Pool.groups = map[*ProcessFactory]*hostGroup{}
	}
	f.group = &hostGroup{opening: make(chan struct{}, f.MaxStarting)}
	f.Pool.groups[f] = f.group
}

func (f *ProcessFactory) Open(ctx context.Context, opts Options) (Runner, error) {
	if err := f.ValidateSession(opts.Config, opts.Provider); err != nil {
		return nil, err
	}
	f.init.Do(f.initialize)
	if f.initErr != nil {
		return nil, f.initErr
	}
	select {
	case f.group.opening <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	defer func() { <-f.group.opening }()
	p := f.Pool
	p.mu.Lock()
	if p.closed {
		p.mu.Unlock()
		return nil, core.E(core.Overloaded, "harness pool is draining")
	}
	var host *process
	var credentialContext [32]byte
	if f.CredentialHosts {
		data, _ := json.Marshal(opts.Provider)
		credentialContext = sha256.Sum256(data)
	}
	total := 0
	factoryTotal := 0
	for factory, group := range p.groups {
		for _, candidate := range group.hosts {
			select {
			case <-candidate.done:
				continue
			default:
			}
			total++
			if factory == f {
				factoryTotal++
			}
			if factory == f && host == nil {
				candidate.mu.Lock()
				available := len(candidate.attachments) < f.MaxShared && candidate.credentialContext == credentialContext
				candidate.mu.Unlock()
				if available {
					host = candidate
				}
			}
		}
	}
	starting := host == nil
	if starting {
		if factoryTotal >= f.MaxHosts || (p.limits.Hosts > 0 && total >= p.limits.Hosts) || (p.limits.Starting > 0 && p.starting >= p.limits.Starting) {
			p.mu.Unlock()
			return nil, core.E(core.Overloaded, "harness host capacity reached")
		}
		var err error
		host, err = f.start(ctx, opts)
		if err != nil {
			p.mu.Unlock()
			return nil, err
		}
		f.group.hosts = append(f.group.hosts, host)
		host.credentialContext = credentialContext
		p.starting++
	}
	a := &attachment{id: core.ID(), host: host, pool: p, opts: opts}
	host.mu.Lock()
	host.attachments[a.id] = a
	host.mu.Unlock()
	p.mu.Unlock()
	if starting {
		defer func() { p.mu.Lock(); p.starting--; p.mu.Unlock() }()
	}
	initCtx, stop := context.WithTimeout(ctx, 30*time.Second)
	defer stop()
	if _, err := a.request(initCtx, "open", opts); err != nil {
		_ = a.Close()
		return nil, err
	}
	return a, nil
}

func (f *ProcessFactory) ValidateSession(config core.Config, provider Provider) error {
	if f.Validate != nil {
		return f.Validate(config, provider)
	}
	return nil
}

func (p *HostPool) beginTurn() error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || (p.limits.ActiveTurns > 0 && p.active >= p.limits.ActiveTurns) {
		return core.E(core.Overloaded, "harness pool active-turn capacity reached")
	}
	p.active++
	return nil
}
func (p *HostPool) endTurn() { p.mu.Lock(); p.active--; p.mu.Unlock() }

func (p *HostPool) Limits() PoolLimits { return p.limits }

// Close drains acquisition and reaps every owned process, including crashed hosts.
// The session engine settles/checkpoints attachments before application shutdown.
func (p *HostPool) Close() error {
	p.mu.Lock()
	p.closed = true
	var hosts []*process
	for _, group := range p.groups {
		hosts = append(hosts, group.hosts...)
	}
	p.mu.Unlock()
	var result error
	for _, host := range hosts {
		result = errors.Join(result, host.Close())
	}
	return result
}
