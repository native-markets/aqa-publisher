# aqa-publisher

Validator sidecar binary to fetch and publish [Aligned Quote Asset (AQA)](https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/aligned-quote-assets) reference rate, used to autonomously collect a deployer's offchain reserve income contribution for distribution to the Hyperliquid protocol.

> [!NOTE]
> [Permissionless spot quote assets](https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/permissionless-spot-quote-assets) enable spot tokens to permisionlessly become quote assets for spot orderbooks (e.g., `HYPE/QUOTE`).
>
> [Aligned quote assets](https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/aligned-quote-assets) are a further primitive to support "aligned stablecoins" that benefit from lower trading fees, better market rebates, and higher volume contribution towards fee tiers when used as quote asset for a spot pair or collateral asset for HIP-3 perps.
>
> A requirement to become an aligned quote asset is that 50% of the deployer’s offchain reserve income must flow to the Hyperliquid protocol. `aqa-publisher` enables network validators to collect and publish the reference rate used to calculate this owed income.

> [!IMPORTANT]
> The current latest release, as of November 19th, 2025, is `v1.0.0` ([GitHub tagged release](https://github.com/native-markets/aqa-publisher/releases/tag/v1.0.0)).

## Setup & Usage

Hyperliquid validators can use the `aqa-publisher` binaries to fetch and report an AQA reference rate, once per day. Three executable binaries are included in this repository:

- [print_current](./src/bin/print_current.rs): Test binary, simply fetches and prints AQA rate to `stdout`
- [publish_once](./src/bin/publish_once.rs): Fetches and publishes AQA rate to network; best if using external scheduler
- [publish_daemon](./src/bin/publish_daemon.rs): Fetches and publishes AQA rate to network, daily at 22:00 UTC

### Environment variables

To use any of the binaries or build approaches, you must first populate a `.env` environment variable file.

By default, only a single `PUBLISHER_PRIVATE_KEY` is required which is the private key of the authorized user who will publish the AQA rate on behalf of your validator to the network.

We recommend provisioning a separate [API/Agent wallet](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets) for this purpose.

```bash
# Copy sample env file and populate
cp .env.example .env
vim .env
```

---

### Build from source locally

#### Prerequisites

To build binaries from source, you will need to have installed the [Rust](https://rust-lang.org/learn/get-started/) toolchain and populated environment variables:

```bash
# Install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Update environment variables
cp .env.example .env
vim .env
```

#### Build binaries

```bash
# Build release binaries
cargo build --release
```

> [!TIP]
> If building on a fresh instance, you will also need necessary linkers and build tooling. The following works for most linux distributions:
> ```bash
> apt-get install build-essential libssl-dev pkg-config -y
> ```

#### Example run (`print_current`)

To simply compute and print the current AQA reference rate, without publishing to the network:

```bash
# Run built binary
./target/release/print_current
```

You should expect to see similar output:

```bash
[2025-11-14T19:50:53Z INFO  print_current] AQA rate on 2025-11-14: 3515319
[2025-11-14T19:50:53Z INFO  print_current] Submission-formatted rate: 0.03515319
```

#### Publishing on-demand (`publish_once`)

To collect the current AQA reference rate and publish to the network once:

```bash
# Run built binary
./target/release/publish_once
```

You should expect to see similar output:

```bash
[2025-11-14T15:31:32Z INFO  aqa_publisher::utils] AQA rate on 2025-11-14: 3515319
[2025-11-14T15:31:32Z INFO  aqa_publisher::utils] Submission-formatted rate: 0.03515319
[2025-11-14T15:31:32Z INFO  aqa_publisher::utils] Loaded publishing signer: 0x...
[2025-11-14T15:31:32Z INFO  aqa_publisher::utils] Publishing to testnet
[2025-11-14T15:31:33Z INFO  aqa_publisher::utils] Validator vote success: Object {"type": String("default")}
```

This is useful to test correct environment variables and setup. During a 24-hour voting period, only your most recent vote is counted.

#### Self-scheduled publishing (`publish_daemon`)

To schedule the continuous collection and publishing of the AQA reference rate, once per day:

```bash
# Run built binary
./target/release/publish_daemon
```

You should expect to see similar output while waiting for execution:

```bash
[2025-11-14T15:32:50Z INFO  publish_daemon] Executed startup data fetch (rate: 3515319 on 2025-11-14)
[2025-11-14T15:32:50Z INFO  publish_daemon] Next scheduled execution: 2025-11-14 22:00:00.000001 UTC (in 6h 27m 9s)
[2025-11-14T15:32:50Z INFO  publish_daemon] Sleeping until next execution
```

And the following output periodically when scheduled execution occurs:

```bash
    --- Scheduled run at 2025-11-13 22:00:00.000000 UTC ---
[2025-11-13 22:00:00Z INFO  publish_daemon] Local time: 2025-11-13 17:00:00.170405 -05:00
[2025-11-13 22:00:00Z INFO  aqa_publisher::utils] AQA rate on 2025-11-13: 3515319
[2025-11-13 22:00:00Z INFO  aqa_publisher::utils] Submission-formatted rate: 3.51531900
[2025-11-13 22:00:00Z INFO  aqa_publisher::utils] Loaded publishing signer: 0x...
[2025-11-13 22:00:00Z INFO  aqa_publisher::utils] Publishing to testnet
[2025-11-13 22:00:00Z INFO  aqa_publisher::utils] Validator vote success: Object {"type": String("default")}
[2025-11-13 22:00:00Z INFO  publish_daemon] Next execution in: 23h 59m 59s
[2025-11-13 22:00:00Z INFO  publish_daemon] Sleeping until next execution
```

---

### Run via Docker locally

#### Prerequisites

To build the Docker image locally, you will need the [Docker](https://docs.docker.com/engine/install/) toolchain and populated environment variables:

```bash
# Install Docker
curl -fsSL https://get.docker.com/ | sh

# Update environment variables
cp .env.example .env
vim .env
```

#### Build image

By default, the [Dockerfile](./Dockerfile) builds and executes the [publish_daemon](./src/bin/publish_daemon.rs) binary executable:

```bash
# In repository root:
docker build -t aqa-publisher .
```

> [!TIP]
> If your user is not part of the docker group, you will need to either execute with elevated privileges:
> ```bash
> sudo docker build -t aqa-publisher .
> ```
> Or, add yourself to the `docker` group:
> ```bash
> sudo usermod -aG docker $USER
> ```

#### Run image

Our recommended approach to run the image locally is via [Docker Compose](https://docs.docker.com/compose/install/):

```bash
docker compose up -d    # Run service
docker compose logs -f  # View logs
docker compose down     # Stop container
```

---

### Using distributed binaries

Native Markets distributes signed `publish_daemon` binaries for common Linux and MacOS architectures. You can download binaries tagged by release [directly from GitHub](https://github.com/native-markets/aqa-publisher/releases).

Our recommendation, still, is to independently build from source to reduce trust assumptions.

#### Verifying signature

Prior to executing untrusted code, we recommend verifying the binaries are signed with the public key `pub_key.asc` found at the root of this repo, to reduce scope of trust to just Native Markets.

All public Native Markets releases are signed by this public key (`all-nm@nativemarkets.com`).

```bash
# Import public key into local keyring
gpg --import pub_key.asc

# Download v1.0.0 binary archive from GitHub release
curl -L -o publish_daemon-macos-arm64.tar.gz https://github.com/native-markets/aqa-publisher/releases/download/v1.0.0/publish_daemon-macos-arm64.tar.gz

# Download v1.0.0 binary archive signature from GitHub release
curl -L -o publish_daemon-macos-arm64.tar.gz.asc https://github.com/native-markets/aqa-publisher/releases/download/v1.0.0/publish_daemon-macos-arm64.tar.gz.asc

# Verify binary
# Using macos-arm64 binary archive as example
gpg --verify \
    publish_daemon-macos-arm64.tar.gz.asc \
    publish_daemon-macos-arm64.tar.gz
```

With a successfully verified binary archive you should expect to see:

```
gpg: Signature made Wed Nov 19 15:07:32 2025 UTC
gpg:                using EDDSA key 0F2980DEE814C761B2016C2F3080B08C4722CF13
gpg: Good signature from "all-nm (Native Markets release publisher) <all-nm@nativemarkets.com>" [unknown]
gpg: WARNING: This key is not certified with a trusted signature!
gpg:          There is no indication that the signature belongs to the owner.
Primary key fingerprint: 0F29 80DE E814 C761 B201  6C2F 3080 B08C 4722 CF13
```

Verifying the valid signature from `all-nm` (you can verify fingerprint `0F29 80DE E814 C761 B201  6C2F 3080 B08C 4722 CF13` and this repo, `native-markets/aqa-publisher`, as source of truth for trusted signature belonging to owner).

#### Executing verified binary

Once verified, you can extract the archive and execute. Ensure `.env` exists.

```bash
tar -xzf publish_daemon-macos-arm64.tar.gz   # Unarchive
chmod +x publish_daemon                      # Make executable
./publish_daemon                             # Run binary
```

---

## Testing

To run unit tests (sans API data collection):

```bash
cargo test --lib
cargo test --test median_aggregator
```

To run integration tests (historic data, two year period) ([source](./tests)):

```bash
cargo test --test average_computation
cargo test --test source_comparison
```

## Scheduled execution

Our default recommendation is to use the [publish_daemon](./src/bin/publish_daemon.rs) binary executable, via Docker, which handles scheduled execution. Should you wish to self-manage, example approaches include `cron` or `systemd`.

For these approaches, make sure to use the [publish_once](./src/bin/publish_once.rs) daemon so scheduling is managed externally.

### `cron` reference

Daily execution at 22:00 UTC:

```cron
0 22 * * * /path/to/aqa_publisher
```

### `systemd` reference

Create a `systemd` timer and service for daily execution at 22:00 UTC.

#### Pre-setup

Build `publish_once` from source and setup directories:

```bash
# Build binary
cargo build --release --bin publish_once

# Setup directories
sudo mkdir -p /opt/aqa_publisher
sudo cp ./target/release/publish_once /opt/aqa_publisher/publish_once

# Verify user groups if running via current user
groups
```

#### `systemd` configuration

Service file `/etc/systemd/system/aqa_publisher.service`:

```ini
[Unit]
Description=AQA Publisher - Publish SOFR reference rate
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=TO_CHANGE_TO_YOUR_USER
Group=TO_CHANGE_TO_YOUR_PREFERRED_GROUP
WorkingDirectory=/opt/aqa_publisher
Environment="PUBLISHER_PRIVATE_KEY=TO_CHANGE"
Environment="NETWORK=testnet"
ExecStart=/opt/aqa_publisher/publish_once
StandardOutput=journal
StandardError=journal

# Security hardening
PrivateTmp=true
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/aqa_publisher

[Install]
WantedBy=multi-user.target
```

Timer file `/etc/systemd/system/aqa_publisher.timer`:

```ini
[Unit]
Description=Daily AQA Publisher execution at 22:00 UTC
Requires=aqa_publisher.service

[Timer]
# Run at 22:00 UTC daily
OnCalendar=*-*-* 22:00:00
Persistent=true
Unit=aqa_publisher.service

[Install]
WantedBy=timers.target
```

Enable and start the timer:

```bash
# Reload systemd configuration
sudo systemctl daemon-reload

# Enable and start the timer
sudo systemctl enable aqa_publisher.timer
sudo systemctl start aqa_publisher.timer

# Check timer status
sudo systemctl status aqa_publisher.timer
sudo systemctl list-timers aqa_publisher.timer

# View logs
sudo journalctl -u aqa_publisher.service -f
```

## Methodology

The [Aligned Quote Asset (AQA) framework](https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/aligned-quote-assets) requires that 50% of the deployer's offchain reserve income must flow to the protocol.

A deployer’s offchain reserve income comes from the aggregate yield of its invested reserves. These reserves may be cash, short-term US treasuries, tokenized US treasury or money market funds, and other low-risk assets as will be defined by applicable regulatory frameworks. Importantly, each investment comes with management fees and investment decisions made in the context of long-term growth.

`aqa-publisher` reports the average trailing 30d SOFR rate as a means of defining a risk-free rate, scaled by a constant that represents industry-standard actual realized rates, subject to change over time via daily validator vote aggregation.

### Data Sources

For redundancy, this rate is collected from three credible sources:

- **[New York Fed](https://markets.newyorkfed.org/static/docs/markets-api.html#/Reference%20Rates)** - Pre-calculated 30-day average from markets API
- **[St. Louis FRED](https://fred.stlouisfed.org/docs/api/fred)** - Pre-calculated 30-day average (SOFR30DAYAVG series)
- **[Office of Financial Research (OFR)](https://www.financialresearch.gov/short-term-funding-monitor/api-specs/api-full-single/)** - Computed from overnight rates using [NY Fed's compounding formula](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates#sofr_ai_calculation_methodology)

The source of truth for the SOFR rate is the New York Fed. Other sources are derivative of this. Multiple sources are used to protect against single source compromise. To maximize transparency, only governmental and quasi-governmental sources with public APIs are used.

### Source Characteristics

The data sources behave slightly differently:

- **NY Fed and FRED**: Commonly report rate next-day and provide 30-day averages directly via API
- **OFR**: Reports daily rate (no 30-day average). Typically delayed till 3 PM Eastern time on day `n+2` (up to 2 days behind)

### Median Aggregation

The 30-day SOFR average is collected (NY Fed, FRED) or computed (OFR) from the data sources with the median of all values used. Aggregation validates:

1. At least 2 sources succeeded in returning data
2. At least one pair of sources agree within 5 basis points (0.05%)

If these conditions are not met, an error is returned. This protects against compromised or incorrect data from any single source.

Rates are returned as scaled `u64` (1% = 1,000,000) with payor-friendly flooring to 8 decimals.

### Source failure

`aqa-publisher` will exit in the following scenarios:

1. **Stale data (>7 days old)**: If the median date from sources is more than 7 days behind query date, the service fails. The 7-day window is generous to handle weekends, holidays, and short government outages, but prevents publishing outdated rates during extended data source failures.
2. **Insufficient source agreement**: If fewer than 2 sources return data, or all pairs of sources differ by more than 5 basis points, the service fails. This protects against compromised or divergent data.
3. **Implausible rate values**: If any source returns a rate outside the range of -5% to 15%, the service fails. These bounds catch parsing errors or compromised data while handling edge cases in extreme market conditions.
4. **Persistent API failures**: If source data collection failure persists, the service exits.

## License

[MIT](./LICENSE)
