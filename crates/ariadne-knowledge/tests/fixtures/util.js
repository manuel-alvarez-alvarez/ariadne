// Halves a number.
function halve(x) {
  return x / 2;
}

const quarter = (x) => halve(halve(x));

test('halves', () => {
  halve(4);
});

module.exports = { halve, quarter };
