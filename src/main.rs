mod desktop;
mod desktop_geo;

use calcelec::{
    load_flow::{self, Options},
    model::{Result, rural_example},
    project::ChangeSet,
    project::Project,
    storage,
};

fn command(args: &[String]) -> Result<()> {
    match args {
        [cmd, path] if cmd == "--unbalanced-load-flow" => {
            let project = storage::read_project(path)?;
            let report = calcelec::unbalanced::study(
                &project,
                &calcelec::unbalanced::Options::default(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .map_err(|e| calcelec::model::Error::Invalid(e.to_string()))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [cmd, path] if cmd == "--load-flow" => run_load_flow(path, false)?,
        [cmd, path, flag] if cmd == "--load-flow" && flag == "--simplified" => {
            run_load_flow(path, true)?
        }
        [cmd, kind, path] if cmd == "--reference-case" => {
            let network = match kind.as_str() {
                "four-wire" => calcelec::reference_cases::four_wire(),
                "transformer" => calcelec::reference_cases::transformer(),
                "dyn-transformer" => calcelec::reference_cases::dyn_transformer(),
                "dyn-grounded" => calcelec::reference_cases::dyn_grounded_transformer(),
                "unbalanced-bt" => calcelec::reference_cases::unbalanced_bt(),
                "grounded-bt" => calcelec::reference_cases::grounded_bt(),
                "source-grounded-bt" => calcelec::reference_cases::source_grounded_bt(),
                _ => {
                    return Err(calcelec::model::Error::Invalid(
                        "référence attendue : four-wire, transformer, dyn-transformer, dyn-grounded, unbalanced-bt, grounded-bt ou source-grounded-bt"
                            .into(),
                    ));
                }
            };
            let project = Project::new(network)?;
            storage::export_new(path, &project.to_json()?)?;
            println!("Cas synthétique créé : {path}");
        }
        [cmd, path] if cmd == "--example" => {
            let project = Project::new(rural_example())?;
            storage::export_new(path, &project.to_json()?)?;
            println!("Exemple créé : {path}\nProjet : {}", project.id);
        }
        [cmd, path] if cmd == "--check" => {
            let project = storage::read_project(path)?;
            let network = project.network()?;
            println!(
                "{}\n{} objets · {} modifications\nVersion : {}\n{} jeux de barres sans alimentation (connectivité)",
                network.name,
                network.objects.len(),
                project.changes.len(),
                project.head(),
                network.unpowered_buses().len()
            );
            let missing = calcelec::electrical::missing_data(&network);
            println!(
                "{} informations électriques détaillées à compléter",
                missing.len()
            );
            for item in missing {
                println!(
                    "{} : {}",
                    network.objects[&item.object_id].name, item.message
                );
            }
        }
        [cmd, input, delta, output] if cmd == "--apply" => {
            let mut project = storage::read_project(input)?;
            let change: ChangeSet = serde_json::from_str(&std::fs::read_to_string(delta)?)?;
            project.apply(change)?;
            storage::export_new(output, &project.to_json()?)?;
            println!("Version reconstruite : {}", project.head());
        }
        _ => println!(
            "CalcElec\n  cargo run                          Application de bureau\n  cargo run -- --example projet.json Créer l'exemple rural\n  cargo run -- --check projet.json   Vérifier un projet\n  cargo run -- --apply base.json lot.json résultat.json\n  cargo run -- --load-flow projet.json [--simplified]  Calcul AC radial, résultats JSON sur stdout\n  cargo run -- --unbalanced-load-flow projet.json  Calcul AC ABCN radial, résultats JSON sur stdout\n  cargo run -- --reference-case four-wire|transformer|dyn-transformer|dyn-grounded|unbalanced-bt|grounded-bt|source-grounded-bt projet.json\nLes exports créent de nouveaux fichiers et refusent de remplacer un fichier existant."
        ),
    }
    Ok(())
}
fn run_load_flow(path: &str, simplified: bool) -> Result<()> {
    let project = storage::read_project(path)?;
    let options = if simplified {
        Options::simplified()
    } else {
        Options::default()
    };
    let report = load_flow::study(
        &project,
        &options,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .map_err(|e| calcelec::model::Error::Invalid(e.to_string()))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]),
            ..Default::default()
        };
        if let Err(error) = eframe::run_native(
            "CalcElec — Réseaux HTA / BT",
            options,
            Box::new(|_| Ok(Box::new(desktop::Desktop::new()))),
        ) {
            eprintln!("Impossible de démarrer l'interface : {error}");
            return std::process::ExitCode::FAILURE;
        }
    } else if let Err(error) = command(&args) {
        eprintln!("{error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
