DASObjectStore Documentation
============================

DASObjectStore is a portable, SSD-ingest-first object appliance for mixed DAS
and NAS-backed storage.

.. toctree::
   :maxdepth: 2
   :caption: User Guide

   user/index

.. toctree::
   :maxdepth: 2
   :caption: Architecture decisions

   adr/index

Design Notes
------------

The repository also contains Markdown design notes covering architecture,
requirements, service orchestration, Mnemosyne integration, metadata recovery,
platform probing, and application authentication and authoritative tokens.
They remain source-controlled design documents rather than operator runbooks.
The local trusted-administrator custody-retention overlay is documented in
``local-trusted-administrator-custody-overlay.md``; it is explicitly a source
contract, not a deployment authorisation.

.. toctree::
   :maxdepth: 2
   :caption: Architecture

   architecture/managed-compute-workspaces
