#!/usr/bin/env nu

use std/assert

def fetch [path: string]: nothing -> record {
  http get --full --allow-errors $path
}

def checks [url: string, log: string] {
  assert equal (fetch $"($url)/ping" | get status) 204
  assert equal (fetch $"($url)/down?bytes=abc" | get status) 400
  assert equal (http delete --full --allow-errors $"($url)/ping" | get status) 405
  assert equal (fetch $"($url)/nope" | get status) 404
  assert equal (http get $"($url)/down?bytes=1000000" | into binary | bytes length) 1000000

  let up = http post --full --allow-errors --content-type application/octet-stream $"($url)/up" (random binary 100_000)
  assert equal ($up | get status) 200

  assert ((http get $"($url)/meta" | get client_ip | str length) > 0)
  assert ((http get $"($url)/" | into string) =~ "HowFastly")
  assert ((fetch $"($url)/ping" | get headers.response | where name == "server-timing" | length) > 0)

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

def main [] {
  let root = git rev-parse --show-toplevel | str trim
  # avoid viceroy's default port since a dev serve session may be running
  let addr = "127.0.0.1:17676"
  let url = $"http://($addr)"

  # dist must exist before compute is built
  do { cd $"($root)/crates/web"; trunk build }

  cargo build -p compute --release --target wasm32-wasip1
  let wasm = $"($root)/target/wasm32-wasip1/release/compute.wasm"
  let log = mktemp -t viceroy-e2e-XXXXXX.log

  # jobs are not killed when a script dies on an error
  # clean up explicitly on both paths
  let server = job spawn { viceroy serve --addr $addr $wasm o+e> $log }

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

  job kill $server
  rm $log

  if $failure != null {
    print -e ($failure.debug? | default $failure.msg)
    exit 1
  }
  print ok
}
