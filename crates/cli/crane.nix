{ crane, ... }:

{
  src = crane.fileSetForCrates [ ../cli ../common ];
}
