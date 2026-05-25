# Stdlib Program Coverage

Each directory below this tree is one executable Go program focused on one
public stdlib package function or method. The runner compares `go run .` output
with `gors run <dir>`.

## fmt

Status: not full. Current fixtures cover 13 of 23 public functions listed by
`go doc fmt`; interface method behavior is not counted in that total and still
needs explicit behavioral fixtures.

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

## strings

Status: not full. Current fixtures cover 47 of 55 package-level functions listed
by `go doc strings`. They do not yet cover constructors or public methods on
`Builder`, `Reader`, and `Replacer`; counting those method surfaces brings the
package to 47 covered targets out of 79 public function/method targets.

Covered direct functions: `Clone`, `Compare`, `Contains`, `ContainsAny`,
`ContainsFunc`, `ContainsRune`, `Count`, `Cut`, `CutPrefix`, `CutSuffix`,
`EqualFold`, `Fields`, `FieldsFunc`, `HasPrefix`, `HasSuffix`, `Index`,
`IndexAny`, `IndexByte`, `IndexFunc`, `IndexRune`, `Join`, `LastIndex`,
`LastIndexAny`, `LastIndexByte`, `LastIndexFunc`, `Map`, `Repeat`, `Replace`,
`ReplaceAll`, `Split`, `SplitAfter`, `SplitAfterN`, `SplitN`, `Title`,
`ToLower`, `ToTitle`, `ToUpper`, `ToValidUTF8`, `Trim`, `TrimFunc`,
`TrimLeft`, `TrimLeftFunc`, `TrimPrefix`, `TrimRight`, `TrimRightFunc`,
`TrimSpace`, and `TrimSuffix`.

Remaining: iterator-returning functions (`FieldsSeq`, `FieldsFuncSeq`,
`Lines`, `SplitSeq`, `SplitAfterSeq`), `unicode.SpecialCase` variants
(`ToLowerSpecial`, `ToTitleSpecial`, `ToUpperSpecial`), `Builder` methods,
`Reader` methods, `Replacer` methods, `NewReader`, and `NewReplacer`.
