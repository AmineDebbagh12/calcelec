mod desktop;

use calcelec::{
    model::{Result, rural_example},
    project::ChangeSet,
    project::Project,
    storage,
};

fn command(args: &[String]) -> Result<()> {
    match args {
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
        }
        [cmd, input, delta, output] if cmd == "--apply" => {
            let mut project = storage::read_project(input)?;
            let change: ChangeSet = serde_json::from_str(&std::fs::read_to_string(delta)?)?;
            project.apply(change)?;
            storage::export_new(output, &project.to_json()?)?;
            println!("Version reconstruite : {}", project.head());
        }
        _ => println!(
            "CalcElec\n  cargo run                          Application de bureau\n  cargo run -- --example projet.json Créer l'exemple rural\n  cargo run -- --check projet.json   Vérifier un projet\n  cargo run -- --apply base.json lot.json résultat.json\nLes exports créent de nouveaux fichiers et refusent de remplacer un fichier existant."
        ),
    }
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
