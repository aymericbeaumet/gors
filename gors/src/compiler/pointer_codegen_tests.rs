use std::path::Path;

use crate::printer;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn write_fixture_file(path: &Path, source: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, source)?;
    Ok(())
}

fn compile_temp_program(dir: &Path) -> TestResult<printer::GeneratedOutput> {
    let dir = dir
        .to_str()
        .ok_or_else(|| std::io::Error::other("temporary fixture path is not valid UTF-8"))?;
    let program = crate::parser::parse_program(dir)?;
    let compiled = super::compile_program_multi(program)?;
    printer::generate_multi(compiled)
}

fn run_generated_rust(output: &printer::GeneratedOutput) -> TestResult {
    let build = tempfile::tempdir()?;
    for (filename, source) in &output.files {
        let path = build.path().join(filename);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, source)?;
    }
    let executable = build.path().join("main");
    let rustc = std::process::Command::new("rustup")
        .args(["run", "1.96.0", "rustc"])
        .arg(build.path().join("main.rs"))
        .args([
            "--edition=2024",
            "-D",
            "unused_imports",
            "-D",
            "unused_macros",
            "-C",
            "overflow-checks=off",
            "-o",
        ])
        .arg(&executable)
        .output()?;
    assert!(
        rustc.status.success(),
        "generated Rust failed to compile:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = std::process::Command::new(executable).output()?;
    assert!(
        run.status.success(),
        "generated Rust failed at runtime:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
fn top_level_values_and_nested_pointer_fields_remain_addressable() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(&tmp.path().join("go.mod"), "module example\n")?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

import "example/state"

func main() { state.Run() }
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("state/state.go"),
        r#"
package state

type Cell struct { value int }

func (c *Cell) Load() int { return c.value }

var Global Cell

type Stamp struct { value int }

func (s *Stamp) Unix() (int, int) { return s.value, 0 }

type Meta struct { Stamp Stamp }
type File struct { Meta Meta }

func shadow(Global Cell) int { return Global.Load() }

func Run() {
	Global.value = 3
	f := &File{Meta: Meta{Stamp: Stamp{value: 4}}}
	seconds, _ := f.Meta.Stamp.Unix()
	if Global.Load()+shadow(Cell{value: 2})+seconds != 9 {
		panic("addressability")
	}
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let state_rs = output
        .files
        .get("example__state.rs")
        .ok_or_else(|| std::io::Error::other("missing generated state module"))?;
    assert!(
        !state_rs.contains("pointer receiver is not addressable"),
        "{state_rs}"
    );
    assert!(
        state_rs.matches("GorsPtr::from_ptr_field").count()
            + state_rs.matches("GorsPtr :: from_ptr_field").count()
            >= 2,
        "expected recursive pointer-backed field projection: {state_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn named_numeric_conversions_preserve_wrappers_and_primitive_boundaries() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(&tmp.path().join("go.mod"), "module example\n")?;
    write_fixture_file(
        &tmp.path().join("units/units.go"),
        r#"
package units

type Mode uint32
type WaitStatus uint32
type Signal int

const High Mode = 1 << 31

func Convert(raw uint16, status WaitStatus) (Mode, Signal) {
	var mode Mode
	mode = Mode(raw & 0777)
	mode |= High
	return mode, Signal(status >> 8)
}
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("reflectlite/reflectlite.go"),
        r#"
package reflectlite

import abi "example/units"

type TFlag = abi.Mode

func Has(flag TFlag) bool { return flag&abi.High != 0 }
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

import (
	"example/reflectlite"
	"example/units"
)

func main() {
	mode, signal := units.Convert(0755, units.WaitStatus(9<<8))
	if uint32(mode&0777) != 0755 || int(signal) != 9 {
		panic("numeric conversion")
	}
	if !reflectlite.Has(mode) {
		panic("named alias identity")
	}
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let units_rs = output
        .files
        .get("example__units.rs")
        .ok_or_else(|| std::io::Error::other("missing generated units module"))?;
    let reflectlite_rs = output
        .files
        .get("example__reflectlite.rs")
        .ok_or_else(|| std::io::Error::other("missing generated reflectlite module"))?;
    assert!(
        !units_rs.contains("isize::from(status)") && !units_rs.contains("isize :: from (status)"),
        "expected named sources to unwrap through their real primitive: {units_rs}"
    );
    assert!(
        !reflectlite_rs.contains("Mode(crate::units::High)")
            && !reflectlite_rs.contains("Mode (crate :: units :: High)"),
        "expected the same imported named type not to be wrapped again: {reflectlite_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn defined_pointer_conversion_projects_shared_underlying_storage() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

type Base struct { value int }
type Alias Base

func (b *Base) SetBase(value int) { b.value = value }
func (b *Base) Value() int { return b.value }
func (a *Alias) Set(value int) { (*Base)(a).SetBase(value) }
func read(a *Alias) int { return (*Base)(a).Value() }

func main() {
	a := &Alias{}
	a.Set(7)
	if read(a) != 7 { panic("pointer conversion lost identity") }
	var nilAlias *Alias
	if (*Base)(nilAlias) != nil { panic("nil pointer conversion") }
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let main_rs = output
        .files
        .get("main.rs")
        .ok_or_else(|| std::io::Error::other("missing generated main module"))?;
    assert!(
        main_rs.contains("GorsPtr::from_ptr_field")
            || main_rs.contains("GorsPtr :: from_ptr_field"),
        "expected a projected pointer view over the source storage: {main_rs}"
    );
    assert!(
        !main_rs.contains("GorsPtr::new((a) as Base)")
            && !main_rs.contains("GorsPtr :: new ((a) as Base)"),
        "expected pointer conversion not to copy or cast the pointer handle: {main_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn addressed_local_index_projects_the_original_array_and_slice_element() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

type cell struct { value int }
type cellArray [2]cell
type cellSlice []cell

func main() {
	var cells [3]cell
	i := 1
	p := &cells[i]
	i = 2
	p.value = 7
	if cells[1].value != 7 || cells[2].value != 0 {
		panic("array element pointer lost its indexed storage")
	}

	values := []cell{{}, {}}
	j := 0
	q := &values[j]
	j = 1
	q.value = 9
	if values[0].value != 9 || values[1].value != 0 {
		panic("slice element pointer lost its indexed storage")
	}

	var namedArray cellArray
	r := &namedArray[1]
	r.value = 11
	if namedArray[1].value != 11 {
		panic("named array element pointer lost its indexed storage")
	}

	namedSlice := cellSlice{{}, {}}
	s := &namedSlice[1]
	s.value = 13
	if namedSlice[1].value != 13 {
		panic("named slice element pointer lost its indexed storage")
	}
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let main_rs = output
        .files
        .get("main.rs")
        .ok_or_else(|| std::io::Error::other("missing generated main module"))?;
    assert!(
        main_rs.matches("from_ptr_field(").count() >= 4,
        "expected address-of indexes to use projected pointer cells: {main_rs}"
    );
    assert!(
        !main_rs.contains("GorsPtr::new((cells)")
            && !main_rs.contains("GorsPtr :: new ((cells)")
            && !main_rs.contains("GorsPtr::new((values)")
            && !main_rs.contains("GorsPtr :: new ((values)"),
        "expected address-of indexes not to copy indexed values: {main_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn variadic_any_boxes_slice_values_without_formatting_them() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

func Hash(data ...any) int {
	stack, ok := data[0].([]uintptr)
	if !ok { panic("slice type erased as another value") }
	return len(stack)
}

func main() {
	stack := []uintptr{1, 2, 3}
	if Hash(stack) != 3 { panic("slice length") }
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let main_rs = output
        .files
        .get("main.rs")
        .ok_or_else(|| std::io::Error::other("missing generated main module"))?;
    assert!(
        !main_rs.contains("format_slice"),
        "expected []T passed to ...any to retain []T as its dynamic value: {main_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn interface_alias_results_box_addressed_concrete_values() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(&tmp.path().join("go.mod"), "module example\n")?;
    write_fixture_file(
        &tmp.path().join("fs/fs.go"),
        r#"
package fs

type Info interface { Name() string }
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("oslike/os.go"),
        r#"
package oslike

import "example/fs"

type Info = fs.Info
type fileStat struct{}

func (f *fileStat) Name() string { return "ok" }

func Stat() (Info, error) {
	var stat fileStat
	return &stat, nil
}
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

import "example/oslike"

func main() {
	info, err := oslike.Stat()
	if err != nil || info.Name() != "ok" { panic("interface alias result") }
}
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let oslike_rs = output
        .files
        .get("example__oslike.rs")
        .ok_or_else(|| std::io::Error::other("missing generated oslike module"))?;
    assert!(
        oslike_rs.contains("Box<dyn crate::fs::Info>")
            || oslike_rs.contains("Box < dyn crate :: fs :: Info >"),
        "expected the alias result to retain the interface ABI: {oslike_rs}"
    );
    run_generated_rust(&output)
}

#[test]
fn mutable_top_level_pointer_vars_are_read_through_their_value_cell() -> TestResult {
    let tmp = tempfile::tempdir()?;
    write_fixture_file(&tmp.path().join("go.mod"), "module example\n")?;
    write_fixture_file(
        &tmp.path().join("setting/setting.go"),
        r#"
package setting

type Setting struct { value string }

func New(value string) *Setting { return &Setting{value: value} }
func (s *Setting) Value() string { return s.value }
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("feature/feature.go"),
        r#"
package feature

import "example/setting"

var Debug = setting.New("first")

func Reset() { Debug = setting.New("second") }

func Run() {
	Reset()
	if Debug.Value() != "second" { panic("mutable pointer package var") }
}
"#,
    )?;
    write_fixture_file(
        &tmp.path().join("main.go"),
        r#"
package main

import "example/feature"

func main() { feature.Run() }
"#,
    )?;

    let output = compile_temp_program(tmp.path())?;
    let feature_rs = output
        .files
        .get("example__feature.rs")
        .ok_or_else(|| std::io::Error::other("missing generated feature module"))?;
    assert!(
        feature_rs.contains("Mutex<crate::builtin::GorsPtr<crate::setting::Setting>>")
            || feature_rs
                .contains("Mutex < crate :: builtin :: GorsPtr < crate :: setting :: Setting > >"),
        "expected a mutable package variable to keep its outer value cell: {feature_rs}"
    );
    run_generated_rust(&output)
}
