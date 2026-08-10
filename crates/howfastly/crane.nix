{ crane, ... }:

{
  src = crane.fileSetForCrates [ ../howfastly ../howfastly-common ];
}
