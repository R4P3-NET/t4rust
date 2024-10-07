# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1] - 2024-10-08
### Changed
- Update nom to version 7
- Update syn to version 2

## [0.3.0] - 2020-10-21
### Changed
- Cleanws directive does not apply anymore to expressions (`<#= #>`), it still applies to blocks (`<# #>`)

## [0.2.0] - 2020-02-16
### Added
- Auto-Escape directive

### Changed
- cleanws directive now also applies to the first directive itself

## [0.1.4] - 2020-02-15
### Changed
- Update proc-macro crates to version 1
- Update nom to version version 5

## [0.1.3] - 2019-03-29
### Added
- The `#[TemplateDebug]` attribute should give better error messages

### Changed
- Rename magic variable `f` to `_fmt`
