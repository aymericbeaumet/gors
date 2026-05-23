# Stdlib Program Coverage

Each directory below this tree is one executable Go program focused on one
public stdlib package function or method. The runner compares `go run .` output
with `gors run <dir>`.

## fmt

Covered in this batch: `Append`, `Appendf`, `Appendln`, `Errorf`, `Fprint`,
`Fprintf`, `Fprintln`, `Print`, `Printf`, `Println`, `Sprint`, `Sprintf`,
`Sprintln`.

Remaining: scanning functions (`Scan`, `Scanf`, `Scanln`, `Fscan`, `Fscanf`,
`Fscanln`, `Sscan`, `Sscanf`, `Sscanln`), `FormatString`, and public interface
method behavior.

## sort

Covered in this batch: `Float64s`, `Float64sAreSorted`, `Ints`,
`IntsAreSorted`, `Strings`, `StringsAreSorted`.

Remaining: `Find`, `IsSorted`, `Reverse`, `Search`, `SearchFloat64s`,
`SearchInts`, `SearchStrings`, `Slice`, `SliceIsSorted`, `SliceStable`, `Sort`,
`Stable`, and exported slice type methods.
