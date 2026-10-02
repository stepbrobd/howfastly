{ clangStdenv, lib, pkgsPrev }:

let
  target = clangStdenv.hostPlatform.rust.rustcTarget;
in
pkgsPrev.trunk.overrideAttrs (prev: {
  env = (prev.env or { }) // {
    "CC_${target}" = lib.getExe' clangStdenv.cc "cc";
    "CXX_${target}" = lib.getExe' clangStdenv.cc "c++";
  };
})
