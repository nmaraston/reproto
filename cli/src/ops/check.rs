use build_spec::{matches, path_resolver, publish_matches, repository, semck_check, simple_config};
use clap::{App, Arg, ArgMatches, SubCommand};
use core::{Context, RpRequiredPackage, Version};
use core::errors::*;
use std::rc::Rc;

pub fn options<'a, 'b>() -> App<'a, 'b> {
    let out = SubCommand::with_name("check").about("Check specifications");

    let out = out.arg(
        Arg::with_name("version")
            .long("version")
            .takes_value(true)
            .help("Override published version with argument"),
    );

    let out = out.arg(Arg::with_name("package").multiple(true));

    out
}

pub fn entry(ctx: Rc<Context>, m: &ArgMatches) -> Result<()> {
    let (manifest, mut env) = simple_config(&ctx, m)?;

    let mut manifest_resolver =
        path_resolver(&manifest)?.ok_or_else(|| "could not setup manifest resolver")?;

    let version_override = if let Some(version) = m.value_of("version") {
        Some(Version::parse(version)
            .map_err(|e| format!("not a valid version: {}: {}", version, e))?)
    } else {
        None
    };

    let packages: Vec<RpRequiredPackage> = m.values_of("package")
        .into_iter()
        .flat_map(|it| it)
        .map(|p| RpRequiredPackage::parse(p).map_err(Into::into))
        .collect::<Result<_>>()?;

    let mut results = Vec::new();

    results.extend(publish_matches(
        manifest_resolver.as_mut(),
        version_override.as_ref(),
        &manifest.publish,
    )?);

    results.extend(matches(
        manifest_resolver.as_mut(),
        version_override.as_ref(),
        &packages,
    )?);

    let mut repository = repository(&manifest)?;

    let mut errors = Vec::new();

    for m in results {
        semck_check(&ctx, &mut errors, &mut repository, &mut env, &m)?;
    }

    if errors.len() > 0 {
        return Err(Error::new("Error when checking").with_suppressed(errors));
    }

    Ok(())
}
