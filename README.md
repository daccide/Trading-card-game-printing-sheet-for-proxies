# Card Printer Professional

![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)
![Platform](https://img.shields.io/badge/platform-Windows-lightgrey.svg)
![License](https://img.shields.io/badge/License-Non_Commercial-red.svg)

**Card Printer Professional** è una soluzione desktop progettata per l'impaginazione automatizzata e l'esportazione tipografica di carte da gioco (proxy, TCG). 

Sviluppato con un'architettura ibrida basata su framework Tauri e backend nativo in Rust, il software è concepito per gestire elaborazioni massive di file immagine, garantendo tempi di esecuzione ottimali e la generazione di documenti pronti per i flussi di stampa professionali.

---

## Architettura e Funzionalità

* **Elaborazione Parallela:** Il rendering e il ridimensionamento delle immagini avvengono in modo asincrono a livello nativo (multi-thread), prevenendo colli di bottiglia e latenze nell'interfaccia utente durante il caricamento di grossi volumi di file.
* **Algoritmi di Resampling Avanzati:** Implementazione di filtri di convoluzione per mantenere l'assoluta nitidezza dei dettagli e dei testi originali in fase di ridimensionamento, evitando perdite di qualità (lossless rendering).
* **Calcolo Tipografico Dinamico:** La gestione dello spazio pagina (formato A4) è calcolata dinamicamente. L'utente ha il controllo millimetrico sulle dimensioni delle carte, sull'abbondanza per il taglio (Bleed) e sulla spaziatura inter-elemento (Gap).
* **Supporto Duplex e Crop Marks:** Predisposizione nativa per la stampa fronte/retro speculare e generazione vettoriale dei segni di taglio conformi agli standard tipografici.
* **Output Standardizzato:** Esportazione diretta in formato PDF compatibile con i profili colore e gli standard di archiviazione moderni.

---

## Installazione (End User)

L'applicativo è attualmente compilato e supportato esclusivamente per sistemi operativi **Windows (x64)**.

1. Accedere alla sezione [Releases](../../releases) di questo repository.
2. Scaricare l'ultima versione dell'installer (file `.msi` o eseguibile standalone).
3. Eseguire il file per avviare il processo di installazione. Non sono richiesti framework aggiuntivi o dipendenze esterne.

---

## Istruzioni per la Compilazione (Developers)

Per i manutentori o per chi desidera compilare il software dal codice sorgente, l'ambiente di sviluppo richiede i seguenti pre-requisiti installati sul sistema host Windows:

- [Node.js](https://nodejs.org/)
- [Rust Toolchain](https://www.rust-lang.org/tools/install)
- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### Setup dell'Ambiente

1. Clonare il repository in locale:
   ```bash
   git clone https://github.com/daccide/Card-Proxy-aligner-CFV-Multi-format.git
   cd card-printer
