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

Covered: `Find`, `Float64s`, `Float64sAreSorted`, `Ints`, `IntsAreSorted`,
`IsSorted`, `Reverse`, `Search`, `SearchFloat64s`, `SearchInts`,
`SearchStrings`, `Slice`, `SliceIsSorted`, `SliceStable`, `Sort`, `Stable`,
`Strings`, and `StringsAreSorted`.

Covered methods: `Float64Slice.Len`, `Float64Slice.Less`,
`Float64Slice.Search`, `Float64Slice.Sort`, `Float64Slice.Swap`,
`IntSlice.Len`, `IntSlice.Less`, `IntSlice.Search`, `IntSlice.Sort`,
`IntSlice.Swap`, `StringSlice.Len`, `StringSlice.Less`, `StringSlice.Search`,
`StringSlice.Sort`, and `StringSlice.Swap`.

Remaining: broader `Interface` implementations beyond `IntSlice`,
`Float64Slice`, and `StringSlice`, plus closure-backed `Slice` comparators once
function literals are supported by the parser/compiler.
