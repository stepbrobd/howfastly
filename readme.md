# HowFastly

Binary Cache:

- Cache: <https://cache.ysun.co>
- Key: `cache.ysun.co-1:WxPYwT5g3kt9XhUhHPpNLZKI9HIOsVVAuqSHpok8Qt4=`

HowFastly measures your connection speed to the [Fastly](https://www.fastly.com)
network, running on
[Fastly Compute](https://www.fastly.com/products/edge-compute). Unlike
[fastly-debug.com](https://fastly-debug.com), which only reports connectivity
diagnostics, HowFastly measures throughput and latency, inspired by
[speed.cloudflare.com](https://speed.cloudflare.com).

- Web: <https://speed.edgecompute.app> (or <https://howfastly.edgecompute.app>)
- CLI: `nix run github:stepbrobd/howfastly#howfastly`

To install with `cargo`:

```sh
RUSTFLAGS='--cfg reqwest_unstable' cargo install howfastly
```

Or run without installing:

```sh
nix run \
  --substituters 'https://cache.ysun.co' \
  --trusted-public-keys 'cache.ysun.co-1:WxPYwT5g3kt9XhUhHPpNLZKI9HIOsVVAuqSHpok8Qt4=' \
  github:stepbrobd/howfastly#howfastly
```

See
[`stepbrobd/inc#howfastly`](https://github.com/stepbrobd/inc/blob/master/pkgs/howfastly/default.nix)
for packaging.

Nix users: multiple public binary cache instances like
[cache.nixos.org](https://cache.nixos.org) and
[cache.ysun.co](https://cache.ysun.co) are fronted by Fastly, so speedtest
results are a fairly accurate representation of how fast binary cache fetches
will run on your connection.

Methodology:

- 25 unloaded latency pings (median, jitter, min, avg)
- Download transfers from 100 kB to 100 MB, uploads from 100 kB to 50 MB
- Per size iteration counts (8/8/6/4/2, larger transfers have lower relative
  variance so fewer repeats)
- 30s time budget per direction (roughly 640 MB transferred worst case)
- Reports p90 speed with loaded latency per direction (bufferbloat)
- Server processing time calculated from samples using `Server-Timing` header
- Reports the serving POP with its name and region, resolved on the edge from
  the Fastly datacenters API
- No packet loss measurement (this would require raw UDP probes, which
  [speed.cloudflare.com](https://speed.cloudflare.com) sends through a WebRTC
  TURN server native to the Cloudflare network, Fastly Compute cannot emit raw
  L3 frames, and Fastly does not have freely usable TURN server)

## GitHub Action

HowFastly can also be used as a composite action that measures a runner's
connection speed to Fastly (note that the workflow downloads a prebuilt binary
from this repository's releases and verifies it against the release checksums,
regardless, you should verify everything carefully). Pinning a release tag runs
that release's binary. Any other ref (either a branch name or a commit SHA),
runs the latest release. Supported runners are Linux x64/arm64 and macOS arm64.

```yaml
jobs:
  speedtest:
    runs-on: ubuntu-latest
    steps:
      - id: howfastly
        uses: stepbrobd/howfastly@master
        with:
          # release tag to install
          # empty uses the pinned tag or the latest release
          version: ""
          # extra CLI flags appended to the default run
          args: -m 10m --ipv4
          # shell command run instead of the default invocation
          # mutually exclusive with args
          # howfastly is on PATH
          # write JSON to $HOWFASTLY_RESULTS to keep outputs and the summary
          # command: howfastly --format json > "$HOWFASTLY_RESULTS"
      - env:
          DOWNLOAD: ${{ steps.howfastly.outputs.download }}
        run: echo "download p90 ${DOWNLOAD} Mbps"
```

Outputs:

- `download`, `upload`: 90th-percentile throughput in Mbps
- `latency`: Median unloaded latency in ms
- `pop`: Three letter code of the Fastly POP that served the test (name and
  region are in `json` under `meta.pop`)
- `json`: Full results as compact JSON
- `results`: Pretty-printed results file, point `actions/upload-artifact` at it
  to keep an artifact

Each run also writes a human-readable summary to the workflow run page.

## Sponsorship disclaimer

Not an official Fastly product or project. This project is generously supported
by [Fastly](https://www.fastly.com) through the
[Fast Forward](https://www.fastly.com/fast-forward) program. The views and
content of this project are solely those of the author and do not imply
endorsement by Fastly.

## License

[Apache-2.0](license.txt)
