#!/usr/bin/env nu

# probe the publication limit of a deployment, three posts per address per 15 minutes
# the posts carry no json, so they are refused before anything is read and nothing is stored
# the probing address is boxed for 15 minutes when the limit is active

def main [url: string] {
  let posts = 1..4 | each {|_|
    http post --full --allow-errors --content-type text/plain $"($url)/share" probe
  }
  let codes = $posts | get status
  if $codes == [415 415 415 429] {
    let wait = $posts | last | get headers.response | where name == retry-after | first | get value
    print $"limited, retry after ($wait) s"
  } else if $codes == [415 415 415 415] {
    print unlimited
  } else {
    error make {msg: $"unexpected statuses ($codes)"}
  }
}
