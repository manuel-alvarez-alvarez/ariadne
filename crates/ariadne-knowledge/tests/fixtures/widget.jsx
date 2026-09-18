// A widget.
export function Widget(props) {
  return <span>{label(props)}</span>;
}

function label(props) {
  return props.name;
}
