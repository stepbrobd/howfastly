# HowFastly

Binary Cache:

- Cache: <https://cache.ysun.co>
- Key: `cache.ysun.co-1:WxPYwT5g3kt9XhUhHPpNLZKI9HIOsVVAuqSHpok8Qt4=`

HowFastly measures your connection speed to the [Fastly](https://www.fastly.com)
network, inspired by [speed.cloudflare.com](https://speed.cloudflare.com).
People keep asking for a Fastly equivalent, and
[fastly-debug.com](https://fastly-debug.com) reports connectivity diagnostics
rather than throughput. A single
[Fastly Compute](https://www.fastly.com/products/edge-compute) service
(wasm32-wasip1) streams the test payloads, answers latency pings, and serves the
Leptos web UI, built to showcase the flexibility of the Compute platform.

- Web: <https://howfastly.edgecompute.app> (or <https://speed.edgecompute.app>)
- CLI: `nix run github:stepbrobd/howfastly#howfastly`

For Nix users: [cache.nixos.org](https://cache.nixos.org) and
[cache.ysun.co](https://cache.ysun.co) are both fronted by Fastly, so these
numbers are a fairly accurate representation of how fast binary cache fetches
will run on your connection.

Methodology (following Cloudflare's):

- 25 unloaded latency pings (median, jitter, min, avg)
- Download and upload transfers from 100 kB to 100 MB, size classes interleaved
  so both directions show estimates early
- Per size iteration counts (8/8/6/4/2), larger transfers have lower relative
  variance and need fewer repeats
- p90 headline speed, per size medians and box plots, loaded latency per
  direction (bufferbloat)
- 30 s time budget per direction, roughly 640 MB transferred worst case
- Server processing time is subtracted from every sample via Server-Timing

## CLI

```
nix run github:stepbrobd/howfastly#howfastly
```

`howfastly` mirrors [cfspeedtest](https://github.com/code-inflation/cfspeedtest)
flags: `--nr-tests` (flat override for the iteration plan),
`--nr-latency-tests`, `--max-payload-size 100k|1m|10m|25m|100m`,
`--download-only`, `--upload-only`,
`--output-format human|json|json-pretty|csv`, and `--url` (or `HOWFASTLY_URL`)
to point at another deployment.

## Development

```
nix develop        # rust toolchain, trunk, viceroy, fastly cli
nix run .#serve    # local simulator on 127.0.0.1:7676
cargo nextest run  # unit, property, and e2e tests
```

Pushes to master build on CI and deploy to Fastly automatically.

## Planned

- HTTP/3. All measurements currently ride TCP (HTTP/2 in browsers, HTTP/1.1 in
  the CLI). The Fastly edge already terminates HTTP/3 for this service
  (`curl --http3-only` gets a 200), but neither an Alt-Svc header nor an HTTPS
  DNS record advertises it, so no real client negotiates QUIC. Nix recently
  gained HTTP/3 support
  ([NixOS/nix#15961](https://github.com/NixOS/nix/pull/15961)) and both caches
  above speak it, so TCP-only results are not representative of h3 cache
  fetches.

## Sponsorship disclaimer

Not an official Fastly product or project. This project is generously supported
by [Fastly](https://www.fastly.com) through the
[Fast Forward](https://www.fastly.com/fast-forward) program. The views and
content of this project are solely those of the author and do not imply
endorsement by Fastly.

## License

[Apache-2.0](license.txt)
