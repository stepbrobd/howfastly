#!/usr/bin/env nu

use std/assert

def fetch [path: string]: nothing -> record {
  http get --full --allow-errors $path
}

def share-payload [] {
  {
    format: 1
    client: cli
    build: e2e
    finished_at: ((date now | into int) // 1_000_000_000)
    config: {
      latency_samples: 1
      download: [{bytes: 100_000, iterations: 1}]
      upload: []
      time_budget_secs: 30.0
    }
    latency: {min: 10.0, avg: 10.0, median: 10.0, jitter: 0.0}
    download: {
      summary: {
        p90: 100.0
        sizes: [{bytes: 100_000, samples: 1, median: 100.0, skipped: false}]
        loaded: null
      }
      samples: null
      timeline: null
    }
    upload: null
  }
}

def sharing [url: string] {
  let payload = share-payload
  let created = http post --full --allow-errors --content-type application/json $"($url)/share" $payload
  assert equal $created.status 201
  let link = $created.body
  assert ($link.id =~ '^[0-9a-f]{64}$')
  assert equal $link.url $"($url)/share/($link.id)"

  let result = fetch $"($link.url).json"
  assert equal $result.status 200
  let report = $result.body
  assert equal $report.download.summary.p90 100.0
  assert equal $report.client cli
  assert equal ($report.expires_at - $report.published_at) 604_800
  assert equal $link.expires_at $report.expires_at
  assert (not ("ip" in ($report.publication | columns)))
  # the record freshness replaces the no-store every other response carries
  let cache = $result.headers.response | where name == cache-control
  assert equal ($cache | length) 1
  let cache = $cache | first | get value
  let age = $cache | parse --regex 'max-age=(?<seconds>[0-9]+)' | first | get seconds | into int
  assert ($age > 0 and $age <= 604_800)
  assert ($cache | str contains immutable)
  assert (not ($cache | str contains no-store))

  # cross a clock tick so an accidental overwrite would change publication time
  sleep 1100ms
  let repeated = http post --full --allow-errors --content-type application/json $"($url)/share" $payload
  assert equal $repeated.status 200
  assert equal $repeated.body $link
  assert equal (http get $"($link.url).json") $report

  # the page is the shell with a head written for this result and the report embedded
  let page = fetch $link.url
  assert equal $page.status 200
  assert equal ($page.headers.response | where name == cache-control | first | get value) no-cache
  # nothing under share is for search indexes, a result is reached by its link
  assert equal ($page.headers.response | where name == x-robots-tag | first | get value) noindex
  assert equal ($result.headers.response | where name == x-robots-tag | first | get value) noindex
  let html = $page.body | into string
  assert ($html | str contains '<title>HowFastly: 100.0 Mbps down, 10.0 ms latency</title>')
  assert ($html | str contains ('<meta property="og:url" content="' + $link.url + '" />'))
  assert ($html | str contains '<script id="howfastly-report" type="application/json">{"format":1,')
  assert equal ($html | split row '<title>' | length) 2
  # a build string cannot close the script element, it reaches the page as json escapes
  let hostile = $payload | upsert build '</script><svg/onload=alert(1)>'
  let planted = http post --full --allow-errors --content-type application/json $"($url)/share" $hostile
  assert equal $planted.status 201
  let html = http get $planted.body.url | into string
  assert (not ($html | str contains '"build":"</script>'))
  assert ($html | str contains '"build":"\u003c/script\u003e\u003csvg/onload=alert(1)\u003e"')
  assert ($html | str contains 'content="Measured ')
  assert (not ($html | str contains '</script><svg'))
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $link.url) "200"
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $"($link.url).json") "200"
  assert equal (fetch $"($url)/share" | get status) 404
  assert equal (fetch $"($url)/share/short.json" | get status) 404
  assert equal (http delete --full --allow-errors $link.url | get status) 405
  assert equal (http delete --full --allow-errors $link.url | get headers.response | where name == allow | first | get value) "GET, HEAD"
  assert equal (http post --full --allow-errors --content-type text/plain $"($url)/share" "nope" | get status) 415
  assert equal (http post --full --allow-errors --content-type application/json $"($url)/share" "nope" | get status) 400
  assert equal (http post --full --allow-errors --content-type application/json $"($url)/share" ($payload | upsert format 2) | get status) 422
  # nu reserializes a json body and would drop the padding, curl sends the bytes
  let oversized = ($payload | to json --raw) ++ ("" | fill --width 65537)
  let refused = $oversized | curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' --data-binary @- $"($url)/share"
  assert equal $refused "413"

  # the seeded record remains in KV despite being past its public expiry
  let expired = $"($url)/share/0000000000000000000000000000000000000000000000000000000000000000"
  let gone = fetch $expired
  assert equal $gone.status 404
  assert equal ($gone.headers.response | where name == x-robots-tag | first | get value) noindex
  assert equal (fetch $"($expired).json" | get status) 404
  let unsupported = $"($url)/share/1111111111111111111111111111111111111111111111111111111111111111"
  assert equal (fetch $unsupported | get status) 422
  assert equal (fetch $"($unsupported).json" | get status) 422
  let missing = fetch $"($url)/share/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff.json"
  assert equal $missing.status 404
  assert equal ($missing.headers.response | where name == cache-control | first | get value) no-store

  # viceroy stubs the rate limiter, the probe must still run and say so
  assert equal (nu ($env.FILE_PWD | path join burst.nu) $url --pause 0sec | str trim) unlimited
}

def checks [url: string, log: string] {
  assert equal (fetch $"($url)/ping" | get status) 204
  assert equal (fetch $"($url)/down?bytes=abc" | get status) 400
  assert equal (http delete --full --allow-errors $"($url)/ping" | get status) 405
  assert equal (http delete --full --allow-errors $"($url)/ping" | get headers.response | where name == allow | first | get value) "GET, HEAD"
  assert equal (fetch $"($url)/nope" | get status) 404

  # a head takes the get path and answers with its headers alone
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $"($url)/") "200"
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $"($url)/ping") "204"
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $"($url)/down?bytes=1000") "200"
  assert equal (curl -s -o /dev/null -w '%{http_code}' -I $"($url)/nope") "404"
  assert ((curl -sI $"($url)/" | str lowercase) =~ "content-type: text/html")
  assert equal (http get $"($url)/down?bytes=1000000" | into binary | bytes length) 1000000

  let up = http post --full --allow-errors --content-type application/octet-stream $"($url)/up" (random binary 100_000)
  assert equal ($up | get status) 200

  let meta = http get $"($url)/meta"
  assert (($meta.ip | str length) > 0)
  # viceroy geolocates the loopback address, the pop lookup has no store locally
  assert equal ($meta.coordinates.latitude | describe) "float"
  assert equal $meta.pop.coordinates null
  # only a nix built wasm knows its store path
  if ($meta.store? | is-not-empty) {
    assert ($meta.store | str starts-with "/nix/store/")
  }
  assert equal (http post --full --allow-errors $"($url)/start" "" | get status) 204
  assert equal (http post --full --allow-errors --content-type application/json $"($url)/finish" {meta: null, latency: null, download: null, upload: null} | get status) 204
  assert equal (http post --full --allow-errors --content-type application/json $"($url)/finish" "nope" | get status) 400
  assert ((http get $"($url)/" | into string) =~ "HowFastly")
  assert ((fetch $"($url)/ping" | get headers.response | where name == "server-timing" | length) > 0)
  sharing $url

  # clients hanging up mid-download are normal for a speed test
  # the guest must stay quiet and the server must keep serving
  # curl aborts mid-body where nu http does not
  curl -so /dev/null --max-time 0.3 $"($url)/down?bytes=1000000000" | complete | ignore
  sleep 500ms
  assert equal (fetch $"($url)/ping" | get status) 204
  assert (not (open $log | str contains "WebAssembly exited with error"))

  # regression guard for quadratic uploads
  # small guest read buffers make viceroy rebuffer the unread remainder
  # on every read, which once turned 50mb uploads into ~40s
  let elapsed = timeit {
    http post --content-type application/octet-stream $"($url)/up" (random binary 50_000_000) | ignore
  }
  assert ($elapsed < 10sec) $"50mb upload took ($elapsed)"
}

# a prebuilt wasm in HOWFASTLY_WASM skips the builds, the nix check hands one in
def wasm []: nothing -> string {
  if "HOWFASTLY_WASM" in $env {
    return $env.HOWFASTLY_WASM
  }
  let root = $env.FILE_PWD | path join .. .. .. | path expand
  # dist must exist before compute is built
  do { cd $"($root)/crates/howfastly-web"; trunk build }
  cargo build -p howfastly-compute --release --target wasm32-wasip1
  $"($root)/target/wasm32-wasip1/release/howfastly-compute.wasm"
}

def main [] {
  # avoid viceroy's default port since a dev serve session may be running
  let addr = "127.0.0.1:17676"
  let url = $"http://($addr)"

  let wasm = wasm
  let log = mktemp -t viceroy-e2e-XXXXXX.log
  let config = mktemp -t viceroy-sharing-XXXXXX.toml
  let manifest = if "HOWFASTLY_CONFIG" in $env {
    $env.HOWFASTLY_CONFIG
  } else {
    $env.FILE_PWD | path join .. .. .. fastly.toml | path expand
  }
  let expired = share-payload | merge {
    published_at: 0
    expires_at: 1
    publication: {
      asn: 0, org: "", city: "", country: "", coordinates: null
      pop: {code: "", name: "", group: "", coordinates: null}
      protocol: "", version: "", cargo: e2e, store: null
    }
  }
  # tests use local stores without sending analytics to the configured backend
  open $manifest
    | upsert local_server.backends {}
    | upsert local_server.kv_stores {kvstore: [{
      key: "0000000000000000000000000000000000000000000000000000000000000000"
      data: ($expired | to json --raw)
    }, {
      key: "1111111111111111111111111111111111111111111111111111111111111111"
      data: '{"format":2}'
    }]}
    | to toml
    | save --force $config

  # jobs are not killed when a script dies on an error
  # clean up explicitly on both paths
  let server = job spawn { viceroy serve --config $config --addr $addr $wasm o+e> $log }

  mut ready = false
  for _ in 1..50 {
    $ready = (try { (fetch $"($url)/ping" | get status) == 204 } catch { false })
    if $ready { break }
    sleep 200ms
  }

  let failure = try {
    assert $ready "viceroy did not come up"
    checks $url $log
    null
  } catch { |err| $err }

  # the job is already gone when viceroy died on its own
  try { job kill $server }
  rm $config

  if $failure != null {
    print -e ($failure.debug? | default $failure.msg)
    print -e "viceroy log:"
    print -e (open $log)
    rm $log
    exit 1
  }
  rm $log
  print ok
}
