package config

import (
	"os"
	"time"

	"github.com/sirupsen/logrus"
)

// Config represents the configuration of a soroban-rpc server
type Config struct {
	ConfigPath string

	Strict bool

	StellarCoreURL         string
	CaptiveCoreUseDB       bool
	CaptiveCoreStoragePath string
	StellarCoreBinaryPath  string
	CaptiveCoreConfigPath  string
	CaptiveCoreHTTPPort    uint

	Endpoint                         string
	AdminEndpoint                    string
	CheckpointFrequency              uint32
	CoreRequestTimeout               time.Duration
	DefaultEventsLimit               uint
	EventLedgerRetentionWindow       uint32
	FriendbotURL                     string
	HistoryArchiveURLs               []string
	IngestionTimeout                 time.Duration
	LogFormat                        LogFormat
	LogLevel                         logrus.Level
	MaxEventsLimit                   uint
	MaxHealthyLedgerLatency          time.Duration
	NetworkPassphrase                string
	PreflightWorkerCount             uint
	PreflightWorkerQueueSize         uint
	SQLiteDBPath                     string
	TransactionLedgerRetentionWindow uint32

	// We memoize these, so they bind to pflags correctly
	optionsCache *ConfigOptions
}

func (cfg *Config) SetValues() error {
	// We start with the defaults
	if err := cfg.loadDefaults(); err != nil {
		return err
	}

	// Then we load from the environment variables and cli flags, to try to find
	// the config file path
	if err := cfg.loadEnv(); err != nil {
		return err
	}
	if err := cfg.loadFlags(); err != nil {
		return err
	}

	// If we specified a config file, we load that
	if cfg.ConfigPath != "" {
		// Merge in the config file flags
		if err := cfg.loadConfigPath(); err != nil {
			return err
		}

		// Load from cli flags and environment variables again, to overwrite what we
		// got from the config file
		if err := cfg.loadEnv(); err != nil {
			return err
		}
		if err := cfg.loadFlags(); err != nil {
			return err
		}
	}

	return nil
}

// loadDefaults populates the config with default values
func (cfg *Config) loadDefaults() error {
	for _, option := range cfg.options() {
		if option.DefaultValue != nil {
			if err := option.setValue(option.DefaultValue); err != nil {
				return err
			}
		}
	}
	return nil
}

// loadEnv populates the config with values from the environment variables
func (cfg *Config) loadEnv() error {
	for _, option := range cfg.options() {
		key, ok := option.getEnvKey()
		if !ok {
			continue
		}
		value, ok := os.LookupEnv(key)
		if !ok {
			continue
		}
		if err := option.setValue(value); err != nil {
			return err
		}
	}
	return nil
}

// loadFlags populates the config with values from the cli flags
func (cfg *Config) loadFlags() error {
	for _, option := range cfg.options() {
		if !option.flag.Changed {
			continue
		}
		if err := option.setValue(option.flag.Value.String()); err != nil {
			return err
		}
	}
	return nil
}

// loadConfigPath loads a new config from a toml file at the given path. Strict
// mode will return an error if there are any unknown toml variables set. Note,
// strict-mode can also be set by putting `STRICT=true` in the config.toml file
// itself.
func (cfg *Config) loadConfigPath() error {
	file, err := os.Open(cfg.ConfigPath)
	if err != nil {
		return err
	}
	defer file.Close()
	return parseToml(file, cfg.Strict, cfg)
}

func (cfg *Config) Validate() error {
	return cfg.options().Validate()
}
