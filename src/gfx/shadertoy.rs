//! ⭐ **UN SHADER SHADERTOY, COLLÉ TEL QUEL** — traduit de GLSL en HLSL pour devenir
//! un motif du dégradé animé (voir `gfx::shader_utilisateur`).
//!
//! DEMANDE (2026-10-10), lot 3 de l'éditeur de shaders : importer des shaders
//! Shadertoy. Un tel shader définit
//! `void mainImage(out vec4 fragColor, in vec2 fragCoord)` et lit `iTime`,
//! `iResolution`… La traduction est celle de **naga** (le traducteur de wgpu) : il
//! connaît les pièges qu'une substitution de texte raterait — l'ordre des produits
//! matriciels (`m * v` devient `mul(v, m)`), les constructeurs à un scalaire
//! (`vec3(1.0)`), les noms réservés de HLSL (`noise` devient `noise_`).
//!
//! **LA FORME DU RÉSULTAT** : le HLSL de naga, plus une fonction `Motif` qui pose
//! les entrées de Shadertoy et appelle `mainImage` — c'est donc un motif comme un
//! autre pour `quad_renderer::source_utilisateur`.
//!
//! ⛔ **CE QUI N'EST PAS PRIS EN CHARGE, ET C'EST DIT** : les textures et les tampons
//! (`iChannel0`…`3`) — un motif n'a ni image ni passe précédente à lire.

/// Les entrées de Shadertoy, déclarées en globales PRIVÉES : naga les écrit en
/// `static`, et `Motif` les pose avant d'appeler `mainImage`. Sur DEUX lignes —
/// `ENTETE_LIGNES` les retire des numéros de ligne des erreurs.
const ENTETE: &str = "#version 450\nfloat iTime; float iTimeDelta; int iFrame; vec3 iResolution; vec4 iMouse; vec4 iDate; float iSampleRate;\n";
const ENTETE_LIGNES: u32 = 2;

/// naga exige un point d'entrée : il appelle `mainImage`, et c'est ce qui en fait
/// une fonction obligatoire. Le HLSL qu'il produit pour ce `main` n'est jamais
/// appelé (le point d'entrée du dégradé est `PSMain`).
const PIED: &str = "\nlayout(location = 0) out vec4 nd_sortie;\nvoid main() { vec4 c = vec4(0.0); mainImage(c, gl_FragCoord.xy); nd_sortie = c; }\n";

/// **LE MOTIF** qui enveloppe `mainImage`. `fragCoord` est le pixel sur le bureau
/// ENTIER (l'écran virtuel, comme les autres motifs), l'origine EN BAS comme dans
/// Shadertoy — d'où le retournement de `y`. `p + DegOrigine` défait le décalage
/// d'origine de `DegMotif` pour retrouver la position sur l'écran.
///
/// La hauteur (`a`), qui sert au relief et à la perspective, est la LUMINANCE :
/// un shader Shadertoy n'en rend pas d'autre.
const ENVELOPPE: &str = r#"
float4 Motif(float2 p, float aspect, float t) {
    float hauteur = DegEcran.w;
    float2 ecran = (p + DegOrigine) * hauteur;
    iResolution = float3(DegEcran.z, hauteur, 1.0);
    iTime = t;
    iTimeDelta = 1.0 / 60.0;
    iFrame = (int)(t * 60.0);
    iMouse = (float4)0;
    iDate = float4(0.0, 0.0, 0.0, t);
    iSampleRate = 44100.0;
    float4 couleur = (float4)0;
    mainImage(couleur, float2(ecran.x, hauteur - ecran.y));
    return float4(couleur.rgb, dot(saturate(couleur.rgb), float3(0.299, 0.587, 0.114)));
}
"#;

/// **CE CODE EST-IL UN SHADER SHADERTOY ?** — il définit `mainImage`. C'est ce qui
/// décide de l'extension du fichier (`.glsl` contre `.hlsl`, voir
/// `shader_utilisateur::ecrire_source`).
pub fn est_shadertoy(code: &str) -> bool {
    code.contains("mainImage")
}

/// **TRADUIT** un shader Shadertoy en un motif HLSL. L'erreur est déjà lisible,
/// avec les numéros de ligne DU CODE COLLÉ (« ligne 3, colonne 5 : … »), la forme
/// que l'éditeur rend cliquable.
pub fn traduire(code: &str) -> Result<String, String> {
    if let Some(canal) = ["iChannel0", "iChannel1", "iChannel2", "iChannel3", "iChannelResolution", "iChannelTime"]
        .into_iter()
        .find(|c| code.contains(c))
    {
        return Err(format!(
            "ce shader lit {canal} : les textures et les tampons de Shadertoy ne sont pas pris en charge \
             (un motif n'a ni image ni passe précédente à lire)"
        ));
    }
    let source = format!("{ENTETE}{code}{PIED}");
    let options = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
    let module = naga::front::glsl::Frontend::default()
        .parse(&options, &source)
        .map_err(|erreurs| {
            erreurs
                .errors
                .iter()
                .take(6)
                .map(|e| situer(e.meta.is_defined().then(|| e.meta.location(&source)), &e.kind.to_string()))
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| {
            let detail = e.spans().find_map(|(_, texte)| (!texte.is_empty()).then(|| texte.clone()));
            let texte = match detail {
                Some(detail) => format!("{} ({detail})", e.as_inner()),
                None => e.as_inner().to_string(),
            };
            situer(e.location(&source), &texte)
        })?;
    let mut options = naga::back::hlsl::Options::default();
    // `ps_5_0` : la cible du dégradé (voir `shader_utilisateur::choisir`).
    options.shader_model = naga::back::hlsl::ShaderModel::V5_0;
    let pipeline = naga::back::hlsl::PipelineOptions::default();
    let mut hlsl = String::new();
    naga::back::hlsl::Writer::new(&mut hlsl, &options, &pipeline)
        .write(&module, &info, None)
        .map_err(|e| format!("la traduction en HLSL a échoué : {e}"))?;
    hlsl.push_str(ENVELOPPE);
    Ok(hlsl)
}

/// Une erreur de naga, ramenée aux lignes du code collé. Une erreur DANS l'entête
/// ou le pied (le code ne définit pas `mainImage` comme il faut) ne désigne aucune
/// ligne de l'utilisateur : elle le dit autrement.
fn situer(lieu: Option<naga::SourceLocation>, texte: &str) -> String {
    let texte = texte.trim();
    match lieu {
        Some(lieu) if lieu.line_number > ENTETE_LIGNES => {
            format!("ligne {}, colonne {} : {texte}", lieu.line_number - ENTETE_LIGNES, lieu.line_position)
        }
        Some(_) | None if texte.contains("mainImage") => {
            "la fonction void mainImage(out vec4 fragColor, in vec2 fragCoord) est introuvable ou mal déclarée".to_string()
        }
        _ => texte.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le motif traduit doit COMPILER avec le reste du dégradé, par le même
    /// compilateur et la même cible que l'application.
    fn compiler(code: &str) -> Result<Vec<u8>, String> {
        let hlsl = traduire(code)?;
        crate::gfx::shader::compiler_pour_l_utilisateur(&crate::gfx::quad_renderer::source_utilisateur(&hlsl), "PSMain", "ps_5_0")
    }

    /// Le modèle « Nouveau shader » de Shadertoy, mot pour mot.
    #[test]
    fn le_modele_de_shadertoy_compile() {
        let code = "void mainImage( out vec4 fragColor, in vec2 fragCoord )\n{\n    vec2 uv = fragCoord/iResolution.xy;\n    vec3 col = 0.5 + 0.5*cos(iTime+uv.xyx+vec3(0,2,4));\n    fragColor = vec4(col,1.0);\n}\n";
        compiler(code).expect("le modèle de Shadertoy doit compiler");
    }

    /// Ce qu'une substitution de texte raterait : une matrice multipliée par un
    /// vecteur, un `#define`, une constante globale, une fonction nommée `noise`
    /// (réservée en HLSL), `mod`, `atan` à deux arguments, une boucle, `iFrame`.
    #[test]
    fn les_tournures_courantes_compilent() {
        let code = r#"
#define PI 3.14159265
const int ETAPES = 6;
mat2 rot(float a) { float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + 1.), u.x), u.y);
}
float fbm(vec2 p) {
    float v = 0.0, a = 0.5;
    for (int i = 0; i < ETAPES; i++) { v += a * noise(p); p = rot(0.5) * p * 2.0; a *= 0.5; }
    return v;
}
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (2.0 * fragCoord - iResolution.xy) / iResolution.y;
    float ang = atan(uv.y, uv.x);
    float v = fbm(uv * 3.0 + iTime * 0.2) + 0.2 * sin(ang * 5.0 + iTime);
    vec3 col = vec3(0.5) + 0.5 * cos(2.0 * PI * (v + vec3(0.0, 0.33, 0.67)));
    col *= smoothstep(1.5, 0.2, length(uv));
    col = mod(col + 0.1 * float(iFrame % 2), 1.0);
    fragColor = vec4(col, 1);
}
"#;
        compiler(code).expect("les tournures courantes doivent compiler");
    }

    /// Une marche de rayons : `break` dans une boucle, `if` imbriqués.
    #[test]
    fn une_marche_de_rayons_compile() {
        let code = r#"
float map(vec3 p) { return min(length(p - vec3(0, 0, 3)) - 1.0, p.y + 1.0); }
vec3 normale(vec3 p) {
    vec2 e = vec2(0.001, 0);
    return normalize(vec3(map(p + e.xyy) - map(p - e.xyy), map(p + e.yxy) - map(p - e.yxy), map(p + e.yyx) - map(p - e.yyx)));
}
void mainImage(out vec4 o, vec2 u) {
    vec2 uv = (u - .5 * iResolution.xy) / iResolution.y;
    vec3 ro = vec3(0), rd = normalize(vec3(uv, 1));
    float t = 0.;
    for (int i = 0; i < 64; i++) { float d = map(ro + rd * t); if (d < .001) break; t += d; if (t > 20.) break; }
    vec3 col = vec3(0);
    if (t < 20.) { vec3 n = normale(ro + rd * t); col = vec3(max(dot(n, normalize(vec3(sin(iTime), 1, -1))), 0.)); }
    o = vec4(col, 1);
}
"#;
        compiler(code).expect("la marche de rayons doit compiler");
    }

    /// L'erreur désigne la ligne DU CODE COLLÉ, pas celle de la source assemblée.
    #[test]
    fn une_erreur_designe_la_ligne_du_code_colle() {
        let code = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    vec2 uv = fragCoord / iResolution.xy;\n    fragColor = vec4(inconnue, uv, 1.0);\n}\n";
        let erreur = traduire(code).unwrap_err();
        assert!(erreur.starts_with("ligne 3, colonne"), "{erreur}");
    }

    #[test]
    fn une_texture_est_refusee_en_le_disant() {
        let code = "void mainImage(out vec4 c, in vec2 f) { c = texture(iChannel0, f / iResolution.xy); }\n";
        let erreur = traduire(code).unwrap_err();
        assert!(erreur.contains("iChannel0"), "{erreur}");
    }

    #[test]
    fn un_motif_hlsl_n_est_pas_un_shader_shadertoy() {
        assert!(!est_shadertoy(crate::gfx::shader_utilisateur::MODELE));
        assert!(est_shadertoy("void mainImage(out vec4 c, in vec2 f) { c = vec4(1); }"));
    }
}
