defmodule Calc do
  def add(a, b) do
    a + b
  end
end

defmodule CalcTest do
  use ExUnit.Case

  test "adds" do
    assert Calc.add(1, 2) == 3
  end
end
