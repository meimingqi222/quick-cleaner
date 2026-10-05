# Agent Note: WMI method uint32 arguments travel as VT_I4

Status: implemented

## Problem

Calling a WMI method (vendor sensor class `LENOVO_OTHER_METHOD.GetFeatureValue(IDs)`) returned `0x80041005` (`WBEM_E_TYPE_MISMATCH`) on every attempt while class reads, object paths and permissions were fine. The MOF declares `uint32`, so the VARIANT was filled as `VT_UI4` — a type WMI refuses.

## Decision

`Wmi::call_number_with_args` fills `Arg::Number` as `VT_I4` unconditionally. WMI only accepts a small automation-compatible subset of VARIANT types: CIM `uint32` and `uint16` are `VT_I4` (bit-cast into the signed `lVal`), `uint64`/`sint64` are `VT_BSTR`. The fourth argument of `Put` stays 0 — when writing to an instance the type comes from the class definition, restating it only adds one more way to mismatch.

## Alternatives considered

Trusting the MOF literal type — that is the bug. Dropping optional arguments instead (`EnumKey`'s `sSubKeyName`) yields `0x80041008` (`WBEM_E_INVALID_PARAMETER`), which looks like a type mismatch but has a different root cause: arguments are all-or-nothing.

## Consequences

Semi-synchronous queries (`WBEM_FLAG_RETURN_IMMEDIATELY`) report failure on `Next()`, not on `ExecQuery`; a negative HRESULT from `Next()` is an error, not end-of-enumeration — otherwise "no permission" masquerades as "no instances". `examples/thermalprobe.rs` carries a self-check that exercises `GetClass → GetMethod → SpawnInstance → Put → ExecMethod` as a normal user, so a broken calling chain is distinguishable from missing vendor permission.

## Verification

- `examples/thermalprobe.rs`（手工自检例程，非自动化测试；入口为 `main`，`StdRegProv` 只是其中的 WMI 类名）

Proved: organic red — the motivating failure was observed on the real machine (every call returned `0x80041005` with fine permissions; recorded in the pitfalls list before this note existed); the thermalprobe self-check reproduces the exact call chain end to end and returns `ReturnValue = 0` with the VT_I4 convention.
