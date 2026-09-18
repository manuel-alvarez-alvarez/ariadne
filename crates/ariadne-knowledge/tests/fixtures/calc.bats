# Adds two numbers.
add() {
  echo $(($1 + $2))
}

@test "adds" {
  run add 1 2
}
