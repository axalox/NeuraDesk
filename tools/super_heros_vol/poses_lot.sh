#!/bin/sh
# poses A et W, 3 tirages, pour les heros donnes ; emblemes imposes pour ceux qui derivent vers un « S »
S="$(dirname "$0")"; P=/c/IAs/ComfyUI/python_embeded/python.exe
NO="No letter, no S shield, no diamond-shaped shield, no oval logo on the chest."
for n in "$@"; do
  case $n in
    16) X="The chest emblem is a small glowing cyan circuit pattern, $NO";;
    17) X="The chest emblem is a glowing rainbow spiral, $NO";;
    18) X="The chest emblem is three glowing horizontal speed lines in lime green, $NO";;
    23) X="The chest emblem is a small green triangle, $NO";;
    29) X="The chest emblem is a glowing green leaf, $NO";;
    *) X="$NO";;
  esac
  (cd "$S" && POSE_EXTRA="$X" $P -I pose.py $n A 3 && POSE_EXTRA="$X" $P -I pose.py $n W 3) || echo "ECHEC $n"
done
