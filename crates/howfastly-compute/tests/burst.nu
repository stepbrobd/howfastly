#!/usr/bin/env nu

# probe the publication limit of a deployment, three posts per address per 15 min
# the posts carry no json, so they are refused before anything is read and nothing is stored
# a box entry is not seen by every server of a pop at once, so the posts are spaced
# the probing address is boxed for 15 min when the limit is active

def main [url: string, --pause: duration = 3sec] {
  for i in 1..6 {
    let post = http post --full --allow-errors --content-type text/plain $"($url)/share" probe
    match $post.status {
      415 => { sleep $pause }
      429 => {
        let wait = $post.headers.response | where name == retry-after | first | get value
        print $"limited after ($i) posts, retry after ($wait) s"
        return
      }
      _ => { error make {msg: $"unexpected status ($post.status)"} }
    }
  }
  print unlimited
}
