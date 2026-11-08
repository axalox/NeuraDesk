// Sonde du canal des grosses données (lot 0 de docs/plans/editeur-image.md).
// Exécutée par le harnais (NEURADESK_DEBUG_SONDE_CANAL=1) dans une page déjà
// ouverte — voir win32::cef_data::SONDE_JS. N'écrit rien, ne touche à rien :
// elle envoie des octets à Rust et compare ce qu'il dit avoir reçu.
(async () => {
  const PREFIXE = "http://neuradesk-data.invalid/";
  const fnv1a32 = (octets) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < octets.length; i++) h = Math.imul(h ^ octets[i], 0x01000193);
    return (h >>> 0).toString(16).padStart(8, "0");
  };
  // Octets pseudo-aléatoires déterministes (xorshift32) : un motif répétitif
  // masquerait un morceau perdu ou dupliqué.
  const octets = (n) => {
    const t = new Uint8Array(n);
    let x = 0x12345678 ^ n;
    for (let i = 0; i < n; i++) { x ^= x << 13; x ^= x >>> 17; x ^= x << 5; t[i] = x & 0xff; }
    return t;
  };
  const envoyer = (route, corps) =>
    fetch(PREFIXE + route, { method: "POST", body: corps, headers: { "Content-Type": "application/octet-stream" } });
  const lignes = [];
  for (const [nom, n] of [["1Ko", 1024], ["2Mo", 2 << 20], ["20Mo", 20 << 20], ["60Mo", 60 << 20]]) {
    try {
      const corps = octets(n);
      const attendue = fnv1a32(corps);
      const t0 = performance.now();
      const r = await envoyer("sonde/" + nom, corps);
      const recu = await r.json();
      const ms = Math.round(performance.now() - t0);
      const ok = recu.taille === n && recu.empreinte === attendue;
      lignes.push(`${nom} ${ok ? "OK" : "ÉCHEC"} envoyé=${n}/${attendue} reçu=${recu.taille}/${recu.empreinte} ignorés=${recu.ignores} ${ms}ms`);
    } catch (e) {
      lignes.push(`${nom} ERREUR ${e}`);
    }
  }
  try {
    await envoyer("sonde/verdict", new TextEncoder().encode(lignes.join(" | ")));
  } catch (e) {
    // Le canal lui-même est mort : il ne reste que l'ancien pour le dire.
    location.href = "neuradesk://sonde-canal/" + encodeURIComponent(String(e));
  }
})();
