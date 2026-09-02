#!/usr/bin/env nu
# natural earth 1:110m land, 1:110m country borders and 1:10m populated places, public domain
# land.txt holds one ring per line and borders.txt one border segment per line
# both as lon,lat pairs rounded to a tenth of a degree
# consecutive points that round together collapse into one
# rings and segments left with fewer than three points are dropped
# places.txt holds min_zoom, lon, lat and name tab separated
# sorted by min_zoom and cut to keep the embedded file small

const land_source = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_land.geojson"
const borders_source = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_admin_0_boundary_lines_land.geojson"
const places_source = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_populated_places_simple.geojson"
const places_zoom = 6.5

def dedup [] {
    let points = $in
    if ($points | length) < 2 {
        return $points
    }
    [($points | first)] ++ ($points | window 2 | where {|w| $w.0 != $w.1 } | each {|w| $w.1 })
}

# every polyline becomes one line of rounded lon,lat pairs
def polylines [] {
    each {|points|
        $points
        | each {|p| $"(($p | first) | math round --precision 1),(($p | last) | math round --precision 1)" }
        | dedup
        | str join " "
    }
    | where {|line| ($line | split row " " | length) >= 3 }
    | str join "\n"
    | $"($in)\n"
}

def land [] {
    http get $land_source
    | from json
    | get features.geometry.coordinates
    | flatten
    | polylines
}

# a multi line string nests one level deeper than a line string
def borders [] {
    http get $borders_source
    | from json
    | get features.geometry
    | each {|g| if $g.type == "MultiLineString" { $g.coordinates } else { [$g.coordinates] } }
    | flatten
    | polylines
}

def places [] {
    http get $places_source
    | from json
    | get features
    | each {|f| {
        zoom: $f.properties.min_zoom
        lon: ($f.geometry.coordinates | first)
        lat: ($f.geometry.coordinates | last)
        name: $f.properties.name
    } }
    | where zoom <= $places_zoom
    | sort-by zoom
    | each {|p| $"($p.zoom | math round --precision 1)\t($p.lon | math round --precision 2)\t($p.lat | math round --precision 2)\t($p.name)" }
    | str join "\n"
    | $"($in)\n"
}

def main [] {
    land | save --force land.txt
    borders | save --force borders.txt
    places | save --force places.txt
}
