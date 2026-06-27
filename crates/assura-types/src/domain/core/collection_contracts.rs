//! T108: Collection contracts (ListOps, sort, filter).

/// Standard collection operation contracts.
#[derive(Debug, Clone)]
pub(crate) struct CollectionContracts {
    contracts: Vec<CollectionContract>,
}

#[derive(Debug, Clone)]
pub(crate) struct CollectionContract {
    pub name: String,
    pub preserves_length: bool,
}

impl CollectionContracts {
    pub fn new() -> Self {
        let contracts = vec![
            CollectionContract {
                name: "sort".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "filter".into(),
                preserves_length: false,
            },
            CollectionContract {
                name: "map".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "reverse".into(),
                preserves_length: true,
            },
            CollectionContract {
                name: "deduplicate".into(),
                preserves_length: false,
            },
        ];
        Self { contracts }
    }

    pub fn lookup(&self, name: &str) -> Option<&CollectionContract> {
        self.contracts.iter().find(|c| c.name == name)
    }
}

impl Default for CollectionContracts {
    fn default() -> Self {
        Self::new()
    }
}
