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
  assert ((http get $"($url)/" | into string) =~ "howfastly")
  assert ((fetch $"($url)/ping" | get headers.response | where name == "server-timing" | length) > 0)

  # clients hanging up mid-download are normal for a speed test: the guest
  # must stay quiet and the server must keep serving (nu http does not
  # abort mid-body, curl does)
  curl -so /dev/null --max-time 0.3 $"($url)/down?bytes=1000000000" | complete | ignore
  sleep 500ms
  assert equal (fetch $"($url)/ping" | get status) 204
  assert (not (open $log | str contains "WebAssembly exited with error"))
}

def main [] {
  let root = git rev-parse --show-toplevel | str trim
  # not viceroy's default port: a dev serve session may be running
  let addr = "127.0.0.1:17676"
  let url = $"http://($addr)"

  cargo build -p compute --release --target wasm32-wasip1
  let wasm = $"($root)/target/wasm32-wasip1/release/compute.wasm"
  let log = mktemp -t viceroy-e2e-XXXXXX.log

  # jobs are NOT killed when a script dies on an error: clean up explicitly
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
