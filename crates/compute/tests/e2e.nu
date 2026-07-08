#!/usr/bin/env nu

use std/assert

def fetch [path: string]: nothing -> record {
  http get --full --allow-errors $path
}

def main [] {
  let root = git rev-parse --show-toplevel | str trim
  let addr = "127.0.0.1:7676"
  let url = $"http://($addr)"

  cargo build -p compute --release --target wasm32-wasip1
  let wasm = $"($root)/target/wasm32-wasip1/release/compute.wasm"

  # background jobs die with the nu process, error paths included
  let server = job spawn { viceroy serve --addr $addr $wasm | ignore }

  mut ready = false
  for _ in 1..50 {
    $ready = (try { (fetch $"($url)/ping" | get status) == 204 } catch { false })
    if $ready { break }
    sleep 200ms
  }
  assert $ready "viceroy did not come up"

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

  job kill $server
  print ok
}
