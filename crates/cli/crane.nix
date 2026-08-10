{ crane, ... }:

{
  src = crane.fileSetForCrates [ ../cli ../howfastly-common ];
}
