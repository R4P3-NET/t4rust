//! # About
//! t4rust is a minimal templating engine, inspired by the [T4](https://docs.microsoft.com/en-us/visualstudio/modeling/code-generation-and-t4-text-templates) syntax.
//!
//! # Example
//! A simple example how to create a template.
//!
//! ```
//! use t4rust_derive::Template;
//!
//! // Add this attribute to use a template
//! #[derive(Template)]
//! // Specify the path to the template file here
//! #[TemplatePath = "./examples/doc_example1.tt"]
//! // Add this attribute if you want to get debug parsing information
//! // This also enables writing temporary files, you might get better error messages.
//! //#[TemplateDebug]
//! struct Example {
//!     // Add fields to the struct you want to use in the template
//!     name: String,
//!     food: String,
//!     num: i32,
//! }
//!
//! fn main() {
//!     // Generate your template by formating it.
//!     let result = format!("{}", Example { name: "Splamy".into(), food: "Cake".into(), num: 3 });
//!     println!("{}", result);
//!#    assert_eq!(result, "Hello From Template!\nMy Name is: Splamy\nI like to eat Cake.\nNum:1\nNum:2\nNum:3\n\n");
//! }
//! ```
//!
//! `doc_example1.tt`:
//! ```text
//! Hello From Template!
//! My Name is: <# write!(_fmt, "{}", self.name)?; #>
//! I like to eat <#= self.food #>.
//! <# for num in 0..self.num { #>Num:<#= num + 1 #>
//! <# } #>
//! ```
//!
//! Output:
//! ```text
//! Hello From Template!
//! My Name is: Splamy
//! I like to eat Cake.
//! Num:1
//! Num:2
//! Num:3
//! ```
//!
//! # Syntax
//!
//! You can simply write rust code within code blocks.
//!
//! Code is written within `<#` and `#>` blocks.
//! If you want to write a `<#` in template text without starting a code block
//! simply write it twice: `<#<#`. Same goes for the `#>` in code blocks.
//! You dont need to duplicate the `<#` within code blocks and `#>` not in
//! template text blocks.
//!
//! You can use `<#= expr #>` to print out a single expression.
//!
//! Maybe you noticed the magical `_fmt` in the template. This variable gives you
//! access to the formatter and e.g. enables you to write functions in your
//! template. `<# write!(_fmt, "{}", self.name)?; #>` is equal to `<#= self.name #>`.
//!
//! **Warning**: Make sure to never create a variable called `_fmt`! You will get
//! weird compiler errors.
//!
//! # Features
//!
//! ## Auto-escaping
//!
//! Use the `escape` directive in your .tt file:
//! ```text
//! <#@ escape function="escape_html" #>`
//! ```
//!
//! And a function with this signature in your code:
//! ```rust
//! fn escape_html(s: &str) -> String {
//!     todo!(); /* Your escaping code here */
//! }
//! ```
//!
//! All expression blocks (e.g. `<#= self.name #>`) will call the escape
//! function before inserted.
//!
//! You can redeclare this directive as many times and where you want in your
//! template to change or disable (with `function=""`) the escape function.

extern crate proc_macro;

use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::Hasher;
use std::io::prelude::*;
use std::option::Option;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::vec::Vec;

use nom::{
	branch::alt,
	bytes::complete::{
		escaped_transform, is_not, tag, take, take_until, take_while,
	},
	character::complete::{alphanumeric1, line_ending, space0},
	combinator::{map, not, opt, peek},
	multi::many0,
	sequence::tuple,
	IResult,
};
use quote::quote;
use syn::Meta::*;
use syn::*;

use crate::TemplatePart::*;

macro_rules! dbg_println {
	($inf:ident) => { if $inf.debug_print { println!(); } };
	($inf:ident, $fmt:expr) => { if $inf.debug_print { println!($fmt); } };
	($inf:ident, $fmt:expr, $($arg:tt)*) => { if $inf.debug_print { println!($fmt, $($arg)*); } };
}

macro_rules! dbg_print {
	($inf:ident) => { if $inf.debug_print { print!(); } };
	($inf:ident, $fmt:expr) => { if $inf.debug_print { print!($fmt); } };
	($inf:ident, $fmt:expr, $($arg:tt)*) => { if $inf.debug_print { print!($fmt, $($arg)*); } };
}

const TEMPLATE_PATH_MACRO: &str = "TemplatePath";
const TEMPLATE_DEBUG_MACRO: &str = "TemplateDebug";

#[proc_macro_derive(Template, attributes(TemplatePath, TemplateDebug))]
pub fn transform_template(
	input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let macro_input = parse_macro_input!(input as DeriveInput);

	let mut path: Option<String> = None;
	let mut info = TemplateInfo::default();

	for attr in &macro_input.attrs {
		match &attr.meta {
			NameValue(MetaNameValue {
				path: p,
				value: syn::Expr::Lit(ExprLit {attrs: _, lit: Lit::Str(lit_str)}),
				..
			}) => {
				if p.get_ident().expect("Attribute with no name")
					== TEMPLATE_PATH_MACRO
				{
					path = Some(lit_str.value());
				}
			}
			Path(name) => {
				if name.get_ident().expect("Attribute with no name")
					== TEMPLATE_DEBUG_MACRO
				{
					info.debug_print = true;
				}
			}
			_ => {}
		}
	}

	// Get template path
	let mut path_absolute =
		PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
	path_absolute.push(&path.unwrap_or_else(|| {
		panic!(
			"Please specify a #[{}=\"<path>\"] atribute with the template \
			 file path.",
			TEMPLATE_PATH_MACRO
		)
	}));
	let path =
		&path_absolute.canonicalize().expect("Could not canonicalize path");
	dbg_println!(
		info,
		"Looking for template in \"{}\"",
		path.to_str().unwrap()
	);

	// Read template file
	let read = read_from_file(path).expect("Could not read file");

	// Parse template file
	let mut data = match parse_all(&mut info, &read) {
		Ok(data) => data,
		Err(e) => {
			return syn::Error::new_spanned(macro_input, format!("Parse error: {}, reason: {}", e.index, e.reason))
				.into_compile_error()
				.into()
		}
	};

	if info.debug_print {
		debug_to_file(path, &data);
	}

	parse_postprocess(&mut data);

	let data = parse_optimize(data);

	// Build code from template
	info = TemplateInfo::default();
	let mut builder = String::new();
	for part in data {
		match part {
			Text(x) => {
				builder.push_str(generate_save_str_print(&x).as_ref());
			}
			Code(x) => {
				builder.push_str(x.as_ref());
			}
			Expr(x) => {
				builder.push_str(generate_expression_print(&x, &info).as_ref());
			}
			Directive(dir) => {
				apply_directive(&mut info, &dir);
			}
		}
	}

	dbg_println!(info, "Generated Code:\n{}", builder);

	let tokens: proc_macro2::TokenStream =
		builder.parse().expect("Parsing template code failed!");

	// Build frame and insert
	let (impl_generics, ty_generics, where_clause) =
		macro_input.generics.split_for_impl();
	let name = &macro_input.ident;
	let path_str = path.to_str().expect("Invalid path");

	let frame = quote! {
		impl #impl_generics ::std::fmt::Display for #name #ty_generics #where_clause {
			fn fmt(&self, _fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
				let _ = include_bytes!(#path_str);
				#tokens
				Ok(())
			}
		}
	};

	// We could return the code now. The problem is that span information are
	// missing and the error messages are awful.
	// So instead, we write to a file and include! this file, which still does
	// not give us nice errors but at least includes source code.
	if !info.debug_print {
		proc_macro::TokenStream::from(frame)
	} else {
		// Unfortunately we have no access to OUT_DIR like build scripts so we
		// try to emulate that partially.

		// Use hash of template path as filename
		let mut hasher = DefaultHasher::new();
		hasher.write(path_str.as_bytes());

		let out_dir = if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR")
		{
			PathBuf::from(target_dir)
		} else {
			let dir = std::env::var("CARGO_MANIFEST_DIR")
				.expect("CARGO_MANIFEST_DIR not set");
			PathBuf::from(dir).join("target")
		};

		let code_path = out_dir
			.join("t4rust")
			.join(&hasher.finish().to_string())
			.with_extension("rs");

		std::fs::create_dir_all(code_path.parent().unwrap())
			.expect("Failed to create output path");

		// Write file
		std::fs::write(&code_path, frame.to_string().as_bytes())
			.expect("Failed to write compiled template");

		let code_path_str = code_path.to_str();
		proc_macro::TokenStream::from(quote! { include!(#code_path_str); })
	}
}

fn generate_expression_print(print_expr: &str, info: &TemplateInfo) -> String {
	if info.print_postprocessor.is_empty() {
		format!("write!(_fmt, \"{{}}\", {})?;\n", print_expr)
	} else {
		format!(
			"{{
			let _s = format!(\"{{}}\", {});
			let _s_transfomed = {}(&_s);
			_fmt.write_str(&_s_transfomed)?;
			}}\n",
			print_expr, info.print_postprocessor
		)
	}
}

fn generate_save_str_print(print_str: &str) -> String {
	let mut max_sharp_count = 0;
	let mut cur_sharp_count = 0;

	for c in print_str.chars() {
		if c == '#' {
			cur_sharp_count += 1;
			max_sharp_count = std::cmp::max(max_sharp_count, cur_sharp_count);
		} else {
			cur_sharp_count = 0;
		}
	}

	let sharps = "#".repeat(max_sharp_count + 1);
	format!("_fmt.write_str(r{1}\"{0}\"{1})?;\n", print_str, sharps)
}

fn read_from_file(path: &Path) -> Result<String, std::io::Error> {
	let mut file = File::open(path)?;
	let mut contents = String::new();
	file.read_to_string(&mut contents)?;
	Ok(contents)
}

fn debug_to_file(path: &Path, data: &[TemplatePart]) {
	let mut pathbuf = PathBuf::new();
	pathbuf.push(path);
	pathbuf.set_extension("tt.out");
	let writepath = pathbuf.as_path();
	if let Ok(mut file) = File::create(writepath) {
		for var in data {
			match *var {
				Code(ref x) => {
					write!(file, "Code:").unwrap();
					file.write_all(x.as_bytes()).unwrap();
				}
				Text(ref x) => {
					write!(file, "Text:").unwrap();
					file.write_all(x.as_bytes()).unwrap();
				}
				Expr(ref x) => {
					write!(file, "Expr:").unwrap();
					file.write_all(x.as_bytes()).unwrap();
				}
				Directive(ref dir) => {
					write!(file, "Dir:{:?}", dir).unwrap();
				}
			}
			writeln!(file).unwrap();
		}
	}
}

/// Transforms template code into an intermediate representation
fn parse_all(
	info: &mut TemplateInfo,
	input: &str,
) -> Result<Vec<TemplatePart>, TemplateError>
{
	let mut builder: Vec<TemplatePart> = Vec::new();
	let mut cur = input;

	dbg_println!(info, "Reading template");

	while !cur.is_empty() {
		let (crest, content) = parse_text(info, cur)?;
		builder.push(Text(content));
		cur = crest;
		dbg_println!(info, "");

		// Read code block
		if let Ok((rest, _)) = expression_start(cur) {
			dbg_print!(info, " expression start");
			let (crest, content) = parse_code(info, rest)?;
			builder.push(Expr(content));
			cur = crest;
		} else if let Ok((rest, _)) = template_directive_start(cur) {
			dbg_print!(info, " directive start");
			let (crest, content) = parse_code(info, rest)?;
			let dir = parse_directive(&content);
			dbg_println!(info, " Directive: {:?}", dir);
			match dir {
				Ok((_, dir)) => {
					apply_directive(info, &dir);
					builder.push(Directive(dir));
				}
				Err(_) => {
					println!("Malformed directive: {}", &content);
					return Err(TemplateError {
						index: 0,
						reason: format!(
							"Could not understand the directive: {}",
							&content
						),
					});
				}
			}
			cur = crest;
		} else if let Ok((rest, _)) = code_start(cur) {
			dbg_print!(info, " code start");
			let (crest, content) = parse_code(info, rest)?;
			builder.push(Code(content));
			cur = crest;
		}

		dbg_println!(info, " Rest: {:?}", &cur);
	}

	dbg_println!(info, "\nTemplate ok!");

	Result::Ok(builder)
}

fn parse_text<'a>(
	info: &TemplateInfo,
	input: &'a str,
) -> Result<(&'a str, String), TemplateError>
{
	let mut content = String::new();
	let mut cur = input;

	loop {
		let read = read_text(cur);
		match read {
			Ok((rest, done)) => {
				content.push_str(&done);
				if rest.is_empty() {
					return Ok((rest, content));
				}
				cur = rest;
				dbg_print!(info, " take text: {:?}", &done);

				if let Ok((rest, _)) = double_code_start(cur) {
					dbg_print!(info, " double-escape");
					content.push_str("<#");

					if rest.is_empty() {
						return Ok((rest, content));
					}
					cur = rest;
				} else if done.is_empty() {
					return Ok((rest, content));
				}
			}
			Err(_) => {
				if let Ok((rest, done)) = till_end(cur) {
					if rest.is_empty() {
						content.push_str(&done);
						return Ok((rest, content));
					}
				}
				panic!(
					"Reached unknown parsing state (!read_text > !till_end)"
				);
			}
		}

		dbg_println!(info, " Rest: {:?}", &cur);
	}
}

fn parse_code<'a>(
	info: &TemplateInfo,
	input: &'a str,
) -> Result<(&'a str, String), TemplateError>
{
	let mut content = String::new();
	let mut cur = input;

	loop {
		match read_code(cur) {
			Ok((rest, done)) => {
				dbg_print!(info, " take code: {:?}", &done);
				content.push_str(&done);
				cur = rest;

				if let Ok((rest, _)) = code_end(cur) {
					dbg_print!(info, " code end");
					return Ok((rest, content));
				} else if let Ok((rest, _)) = double_code_end(cur) {
					dbg_print!(info, " double-escape");
					content.push_str("#>");
					cur = rest;
				} else {
					panic!("Nothing, i guess?");
				}
			}
			Err(err) => {
				dbg_println!(info, "Error at code {:?}", err);
				return Err(TemplateError {
					index: 0,
					reason: "Unclosed code or expression block".into(),
				});
			}
		}
	}
}

/// Merges multiple identical Parts into one
fn parse_optimize(data: Vec<TemplatePart>) -> Vec<TemplatePart> {
	let mut last_type = TemplatePartType::None;
	let mut combined = Vec::<TemplatePart>::new();
	let mut tmp_build = String::new();
	for item in data {
		match item {
			Code(u) => {
				if u.is_empty() {
					continue;
				}
				if last_type != TemplatePartType::Code {
					if !tmp_build.is_empty() {
						match last_type {
							TemplatePartType::None | TemplatePartType::Code => {
								panic!()
							}
							TemplatePartType::Text => {
								combined.push(Text(tmp_build))
							}
							TemplatePartType::Expr => {
								combined.push(Expr(tmp_build))
							}
						}
					}
					tmp_build = String::new();
					last_type = TemplatePartType::Code;
				}
				tmp_build.push_str(&u);
			}
			Text(u) => {
				if u.is_empty() {
					continue;
				}
				if last_type != TemplatePartType::Text {
					if !tmp_build.is_empty() {
						match last_type {
							TemplatePartType::None | TemplatePartType::Text => {
								panic!()
							}
							TemplatePartType::Code => {
								combined.push(Code(tmp_build))
							}
							TemplatePartType::Expr => {
								combined.push(Expr(tmp_build))
							}
						}
					}
					tmp_build = String::new();
					last_type = TemplatePartType::Text;
				}
				tmp_build.push_str(&u);
			}
			Expr(u) => {
				if !tmp_build.is_empty() {
					match last_type {
						TemplatePartType::None => panic!(),
						TemplatePartType::Code => {
							combined.push(Code(tmp_build))
						}
						TemplatePartType::Text => {
							combined.push(Text(tmp_build))
						}
						TemplatePartType::Expr => {
							combined.push(Expr(tmp_build))
						}
					}
				}
				tmp_build = String::new();
				last_type = TemplatePartType::Expr;
				tmp_build.push_str(&u);
			}
			Directive(d) => {
				combined.push(Directive(d));
			}
		}
	}
	if !tmp_build.is_empty() {
		match last_type {
			TemplatePartType::None => {}
			TemplatePartType::Code => combined.push(Code(tmp_build)),
			TemplatePartType::Text => combined.push(Text(tmp_build)),
			TemplatePartType::Expr => combined.push(Expr(tmp_build)),
		}
	}
	combined
}

/// Applies template directives like 'cleanws' and modifies the input
/// accordingly.
fn parse_postprocess(data: &mut Vec<TemplatePart>) {
	let mut info = TemplateInfo::default();
	let mut was_b_clean = None;
	let mut clean_index = 0;

	// if there are less than 3 blocks available we can't do any transformations
	if data.len() < 3 {
		return;
	}

	for i in 0..(data.len() - 2) {
		let tri = data[i..(i + 3)].as_mut();
		if let Directive(ref dir) = tri[1] {
			apply_directive(&mut info, dir);
		}

		if !info.clean_whitespace
			|| !tri[0].is_text()
			|| !tri[1].should_trim_whitespace()
			|| !tri[2].is_text()
		{
			continue;
		}

		let mut res_a = None;
		if clean_index == i && was_b_clean.is_some() {
			res_a = was_b_clean;
		} else if let Text(ref text_a) = tri[0] {
			let rev_txt: String = text_a.chars().rev().collect();
			if let Ok((_, a_len)) = is_ws_till_newline(&rev_txt) {
				res_a = Some(a_len);
			} else if i == 0 && text_a.is_empty() {
				// Start of file
				res_a = Some((0, 0));
			} else {
				continue;
			}
		}

		let mut res_b = None;
		if let Text(ref text_b) = tri[2] {
			if let Ok((_, b_len)) = is_ws_till_newline(&text_b) {
				res_b = Some(b_len);
			} else {
				continue;
			}
		}

		// start trimming

		if let Text(ref mut text_a) = tri[0] {
			let res_a = res_a.unwrap();
			let len = text_a.len();
			text_a.drain((len - (res_a.0))..len);
		}

		if let Text(ref mut text_b) = tri[2] {
			let rev_txt: String = text_b.chars().rev().collect();
			if let Ok((_, b_len)) = is_ws_till_newline(&rev_txt) {
				was_b_clean = Some(b_len);
				clean_index = i + 2;
			}

			let res_b = res_b.unwrap();
			text_b.drain(0..(res_b.0 + res_b.1));
		}
	}
}

fn apply_directive(info: &mut TemplateInfo, directive: &TemplateDirective) {
	for (key, value) in directive
		.params
		.iter()
		.map(|p| ((directive.name.as_str(), p.0.as_str()), p.1.as_str()))
	{
		match key {
			("template", "debug") => {
				info.debug_print = value.parse::<bool>().unwrap()
			}
			("template", "cleanws") | ("template", "clean_whitespace") => {
				info.clean_whitespace = value.parse::<bool>().unwrap()
			}
			("escape", "function") => {
				info.print_postprocessor = value.to_string()
			}
			_ => println!(
				"Unrecognized template parameter \"{}\" in \"{}\"",
				key.0, key.1
			),
		}
	}
}

// NOM DECLARATIONS ===========================================================

fn expression_start(s: &str) -> IResult<&str, &str> { tag("<#=")(s) }
fn template_directive_start(s: &str) -> IResult<&str, &str> { tag("<#@")(s) }
fn read_text(s: &str) -> IResult<&str, &str> { take_until("<#")(s) }

fn code_start(s: &str) -> IResult<&str, &str> {
	let (s, r) = tag("<#")(s)?;
	not(tag("<#"))(s)?;
	Ok((s, r))
}
fn double_code_start(s: &str) -> IResult<&str, &str> { tag("<#<#")(s) }

fn code_end(s: &str) -> IResult<&str, &str> {
	let (s, r) = tag("#>")(s)?;
	not(tag("#>"))(s)?;
	Ok((s, r))
}
fn double_code_end(s: &str) -> IResult<&str, &str> { tag("#>#>")(s) }

fn read_code(s: &str) -> IResult<&str, &str> { take_until("#>")(s) }

fn till_end(s: &str) -> IResult<&str, &str> { take_while(|_| true)(s) }

fn parse_directive(s: &str) -> IResult<&str, TemplateDirective> {
	map(
		tuple((space0, alphanumeric1, many0(parse_directive_param), at_end)),
		|t| TemplateDirective { name: t.1.to_string(), params: t.2 },
	)(s)
}

fn at_end(s: &str) -> IResult<&str, ()> { not(peek(take(1usize)))(s) }

fn parse_directive_param(s: &str) -> IResult<&str, (String, String)> {
	map(
		tuple((
			space0,
			alphanumeric1,
			space0,
			tag("="),
			space0,
			tag("\""),
			opt(escaped_transform(
				is_not("\\\""),
				'\\',
				alt((tag_transform("\\", "\\"), tag_transform("\"", "\""))),
			)),
			tag("\""),
			space0,
		)),
		|t| (t.1.to_string(), t.6.unwrap_or_else(|| "".to_string())),
	)(s)
}

fn is_ws_till_newline(s: &str) -> IResult<&str, (usize, usize)> {
	map(
		tuple((space0, line_ending)),
		|t: (&str, &str)| (t.0.len(), t.1.len()),
	)(s)
}

fn tag_transform<'a>(
	s: &'a str,
	t: &'a str,
) -> impl Fn(&'a str) -> IResult<&str, &str>
{
	move |i: &'a str| {
		let (r, _) = tag(s)(i)?;
		Ok((r, t))
	}
}

// NOM END ====================================================================

#[derive(Debug)]
struct TemplateError {
	reason: String,
	index: usize,
}

#[derive(Debug)]
struct TemplateDirective {
	name: String,
	params: Vec<(String, String)>,
}

#[derive(Debug)]
enum TemplatePart {
	Text(String),
	Code(String),
	Expr(String),
	Directive(TemplateDirective),
}

impl TemplatePart {
	fn is_text(&self) -> bool { matches!(self, Text(_)) }

	/// Whitespace should only be trimmed for code and directive blocks, we want to keep it for
	/// expressions.
	fn should_trim_whitespace(&self) -> bool { matches!(self, Code(_) | Directive(_)) }
}

#[derive(PartialEq)]
enum TemplatePartType {
	None,
	Code,
	Text,
	Expr,
}

#[derive(Debug)]
struct TemplateInfo {
	debug_print: bool,
	clean_whitespace: bool,
	print_postprocessor: String,
}

impl TemplateInfo {
	fn default() -> Self {
		Self {
			debug_print: false,
			clean_whitespace: false,
			print_postprocessor: "".into(),
		}
	}
}
